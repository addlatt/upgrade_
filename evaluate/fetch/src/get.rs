//! `upgrade-fetch get`: download one release onto the stick it runs from
//! (roadmap item 24; decided 2026-11-07 and 2026-11-08, the owner).
//!
//! The order, and why:
//!
//! 1. Which stick, and is it free to change (stick.rs). Then the plan from
//!    the compiled table (plan.rs).
//! 2. Ask each address how big it is. Refuse if the stick has not the room.
//! 3. Remove the Linux files already on the stick (one release per stick).
//! 4. Each ISO streams through: every byte into one sha256, the files the
//!    stick needs into `upgrade_\fetch\` (iso.rs). A dropped connection
//!    carries on from the byte it stopped at (an HTTP range), up to
//!    `ATTEMPTS` times.
//! 5. The whole ISO's sha256 must be the table's; each file the table
//!    measured must be its sha256; each file is read back from the stick and
//!    must be the bytes that were written (RISKS R17's rule, here too).
//! 6. Only then the files move into place, release.json and the boot-chain
//!    files are written, and SHA256SUMS last.
//!
//! Every refusal removes `upgrade_\fetch\` and says REFUSED. Nothing in it
//! touches this computer's own disk: every write is under the stick's root.

use crate::iso::{Extractor, Want};
use crate::plan::Plan;
use crate::stick::{self, StickFacts};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub const ATTEMPTS: u32 = 6;
/// Room left on the stick after the download, past the ISOs' own sizes.
pub const MARGIN: u64 = 64 * 1024 * 1024;

pub struct Response {
    pub status: u16,
    /// Content-Length
    pub length: Option<u64>,
    /// the first byte of a 206's Content-Range
    pub range_start: Option<u64>,
    pub body: Box<dyn Read>,
}

/// The network, behind a seam: WinHTTP on Windows (net.rs), a made-up
/// server in the tests.
pub trait Net {
    /// GET `url`, from byte `from` (a Range request when from > 0).
    fn get(&mut self, url: &str, from: u64) -> Result<Response, String>;
}

/// What the flow needs from the machine besides the files under the root.
pub trait Machine {
    fn stick_facts(&self) -> StickFacts;
    fn free_bytes(&self, root: &Path) -> Result<u64, String>;
    /// wait before the next try (seconds); the tests do not wait
    fn pause(&mut self, secs: u64);
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// the stick held it already
    AlreadyThere,
    Fetched { bytes: u64 },
}

/// A file being written, with the sha256 of what went in.
struct HashFile {
    out: std::io::BufWriter<std::fs::File>,
    hash: Sha256,
    bytes: u64,
}
#[derive(Clone)]
struct Shared(Rc<RefCell<HashFile>>);
impl Write for Shared {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        let mut h = self.0.borrow_mut();
        h.out.write_all(b)?;
        h.hash.update(b);
        h.bytes += b.len() as u64;
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        let mut h = self.0.borrow_mut();
        h.out.flush()?;
        h.out.get_ref().sync_all()
    }
}

pub fn sha256_file(p: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("{}: could not read it back ({e})", p.display()))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

/// `--from`: a developer's copy of the same files (the rig's local server).
/// Only the folder changes; the file name and every sha256 stay the table's.
pub fn rebase(url: &str, from: Option<&str>) -> String {
    match from {
        Some(base) => format!("{}/{}", base.trim_end_matches('/'), url.rsplit('/').next().unwrap_or("")),
        None => url.to_string(),
    }
}

/// `bytes 1000-4999/5000` -> 1000
pub fn range_start(content_range: &str) -> Option<u64> {
    content_range.trim().strip_prefix("bytes ")?.split('-').next()?.trim().parse().ok()
}

fn mib(b: u64) -> u64 {
    b / (1024 * 1024)
}

/// The flow. `say` gets each line the person (and the log) sees; progress
/// lines are `fetch: progress <done> <total>` in bytes.
pub fn get(root: &Path, plan: &Plan, from: Option<&str>, net: &mut dyn Net, m: &mut dyn Machine, say: &mut dyn FnMut(&str)) -> Result<Outcome, String> {
    let facts = m.stick_facts();
    if let Some(why) = stick::identity_refusal(&facts, root) {
        return Err(why);
    }
    say(&format!("fetch: {} {} onto the stick {} ({})", plan.name, plan.desktop_label, facts.drive, facts.file_system));
    if stick::holds(root, plan).is_ok() {
        say("fetch: this stick already carries it; nothing to download");
        return Ok(Outcome::AlreadyThere);
    }
    if let Some(b) = from {
        if !(b.starts_with("http://") || b.starts_with("https://")) {
            return Err(format!("--from must be an http or https address ({b})"));
        }
        say(&format!("fetch: from {b} (a developer's copy; every byte is still checked against the table)"));
    }

    // 2. sizes and room
    let mut sizes = vec![];
    for p in &plan.parts {
        let url = rebase(&p.url, from);
        let r = net.get(&url, 0).map_err(|e| format!("could not reach {url}: {e}"))?;
        if r.status != 200 {
            return Err(format!("{url} answered {} instead of the file", r.status));
        }
        let Some(len) = r.length.filter(|l| *l > 0) else { return Err(format!("{url} did not say how big it is")) };
        sizes.push(len);
    }
    let total: u64 = sizes.iter().sum();
    let sums_text = std::fs::read_to_string(root.join("SHA256SUMS")).map_err(|e| format!("SHA256SUMS: {e}"))?;
    let old = stick::owned_on_stick(root, &sums_text);
    let old_bytes: u64 = old.iter().filter_map(|p| std::fs::metadata(stick::under(root, p)).ok()).map(|md| md.len()).sum();
    let free = m.free_bytes(root)?;
    if free + old_bytes < total + MARGIN {
        return Err(format!("the stick has room for {} MB and this needs {} MB; use a bigger stick", mib(free + old_bytes), mib(total + MARGIN)));
    }
    say(&format!("fetch: {} MB to download; {} MB free on the stick", mib(total), mib(free + old_bytes)));

    // 3. one release per stick
    if !old.is_empty() {
        say(&format!("fetch: removing the {} Linux files already on this stick", old.len()));
        for p in &old {
            let f = stick::under(root, p);
            if f.exists() {
                std::fs::remove_file(&f).map_err(|e| format!("could not remove {p}: {e}"))?;
            }
        }
        // SHA256SUMS no longer names them, so a stick stopped here carries no half release
        write_replace(root, "SHA256SUMS", stick::merge_sums(&sums_text, &[]).as_bytes())?;
    }
    let staging = root.join("upgrade_").join("fetch");
    let _ = std::fs::remove_dir_all(&staging);
    let r = fetch_all(root, &staging, plan, from, &sizes, net, m, say);
    if r.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    r.map(|_| Outcome::Fetched { bytes: total })
}

fn write_replace(root: &Path, rel: &str, bytes: &[u8]) -> Result<(), String> {
    let p = stick::under(root, rel);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let tmp = p.with_extension("fetch-new");
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        f.write_all(bytes).and_then(|_| f.sync_all()).map_err(|e| format!("{}: {e}", tmp.display()))?;
    }
    std::fs::rename(&tmp, &p).map_err(|e| format!("{rel}: {e}"))?;
    match std::fs::read(&p) {
        Ok(b) if b == bytes => Ok(()),
        _ => Err(format!("{rel} did not read back as written")),
    }
}

#[allow(clippy::too_many_arguments)]
fn fetch_all(root: &Path, staging: &Path, plan: &Plan, from: Option<&str>, sizes: &[u64], net: &mut dyn Net, m: &mut dyn Machine, say: &mut dyn FnMut(&str)) -> Result<(), String> {
    let total: u64 = sizes.iter().sum();
    let mut done_before = 0u64;
    let mut placed: Vec<(String, String, PathBuf)> = vec![];
    for (p, &size) in plan.parts.iter().zip(sizes) {
        let url = rebase(&p.url, from);
        let name = url.rsplit('/').next().unwrap_or("").to_string();
        say(&format!("fetch: getting {name} ({} MB)", mib(size)));
        let mut files = vec![];
        let mut wants = vec![];
        for x in &p.extracts {
            let path = stick::under(staging, &x.dest);
            std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| format!("{}: {e}", staging.display()))?;
            let f = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let h = Shared(Rc::new(RefCell::new(HashFile { out: std::io::BufWriter::with_capacity(1 << 20, f), hash: Sha256::new(), bytes: 0 })));
            wants.push(Want { path: x.iso_path.clone(), out: Box::new(h.clone()) });
            files.push((x.clone(), path, h));
        }
        let mut x = Extractor::new(wants);
        let mut whole = Sha256::new();
        let mut got = 0u64;
        let mut tries = 0u32;
        let mut last_shown = u64::MAX;
        let mut buf = vec![0u8; 1 << 20];
        'attempt: while got < size {
            tries += 1;
            if tries > ATTEMPTS {
                return Err(format!("the download of {name} kept failing ({} of {} MB); check the internet connection and try again", mib(got), mib(size)));
            }
            if tries > 1 {
                let wait = 2u64.pow(tries - 1).min(30);
                say(&format!("fetch: the connection dropped at {} MB; trying again in {wait} s (try {tries} of {ATTEMPTS})", mib(got)));
                m.pause(wait);
            }
            let mut r = match net.get(&url, got) {
                Ok(r) => r,
                Err(e) => {
                    say(&format!("fetch: {e}"));
                    continue 'attempt;
                }
            };
            let skip = match (got, r.status) {
                (0, 200) => 0,
                (g, 206) if r.range_start == Some(g) => 0,
                // a server that ignores the range sends it all again: skip what is already here
                (g, 200) => g,
                (_, s) => {
                    say(&format!("fetch: {url} answered {s}"));
                    continue 'attempt;
                }
            };
            let expect = if r.status == 206 { size - got } else { size };
            if r.length.is_some_and(|l| l != expect) {
                return Err(format!("{url} changed size while it was downloaded ({} bytes, not {expect}); refused", r.length.unwrap_or(0)));
            }
            let mut skipped = 0u64;
            loop {
                let n = match r.body.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) => {
                        say(&format!("fetch: {e}"));
                        continue 'attempt;
                    }
                };
                let mut chunk = &buf[..n];
                if skipped < skip {
                    let s = ((skip - skipped) as usize).min(chunk.len());
                    skipped += s as u64;
                    chunk = &chunk[s..];
                    if chunk.is_empty() {
                        continue;
                    }
                }
                if got + chunk.len() as u64 > size {
                    return Err(format!("{url} sent more bytes than it said it had; refused"));
                }
                whole.update(chunk);
                x.feed(chunk).map_err(|e| format!("{name}: {e}"))?;
                got += chunk.len() as u64;
                let shown = (done_before + got) * 100 / total.max(1);
                if shown != last_shown {
                    last_shown = shown;
                    say(&format!("fetch: progress {} {}", done_before + got, total));
                }
            }
        }
        let hex = format!("{:x}", whole.finalize());
        if hex != p.sha256 {
            return Err(format!("the download of {name} did not match its measured sha256 (got {hex}); nothing was kept"));
        }
        x.finish().map_err(|e| format!("{name}: {e}"))?;
        say(&format!("fetch: {name} matches its measured sha256; reading the files back from the stick"));
        for (ex, path, h) in files {
            let (wrote, n) = {
                let hf = h.0.borrow();
                (format!("{:x}", hf.hash.clone().finalize()), hf.bytes)
            };
            if let Some(want) = &ex.sha256 {
                if &wrote != want {
                    return Err(format!("{} is not the measured file (got {wrote}); nothing was kept", ex.dest));
                }
            }
            drop(h);
            let back = sha256_file(&path)?;
            if back != wrote {
                return Err(format!("{} did not read back from the stick as it was written ({n} bytes); the stick may be failing (RISKS R17)", ex.dest));
            }
            placed.push((ex.dest.clone(), wrote, path));
        }
        done_before += size;
    }

    // 6. into place, then the stick's own record of it, SHA256SUMS last
    for (dest, _, from_path) in &placed {
        let to = stick::under(root, dest);
        std::fs::create_dir_all(to.parent().unwrap()).map_err(|e| format!("{dest}: {e}"))?;
        std::fs::rename(from_path, &to).map_err(|e| format!("could not move {dest} into place: {e}"))?;
    }
    let _ = std::fs::remove_dir_all(staging);
    let mut lines: Vec<(String, String)> = placed.iter().map(|(d, h, _)| (d.clone(), h.clone())).collect();
    let mut small: Vec<(String, String)> = vec![("release.json".into(), plan.release_json.clone())];
    small.extend(plan.boot_chain.iter().cloned());
    for (rel, text) in &small {
        write_replace(root, rel, text.as_bytes())?;
        lines.push((rel.clone(), format!("{:x}", Sha256::digest(text.as_bytes()))));
    }
    let sums = std::fs::read_to_string(root.join("SHA256SUMS")).map_err(|e| format!("SHA256SUMS: {e}"))?;
    write_replace(root, "SHA256SUMS", stick::merge_sums(&sums, &lines).as_bytes())?;
    stick::holds(root, plan).map_err(|e| format!("after the download the stick still does not hold it: {e}"))?;
    say(&format!("fetch: ready: {} {} is on the stick, every file checked", plan.name, plan.desktop_label));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iso::tests::make_iso;
    use crate::plan::{Extract, Part};
    use crate::stick::DriveKind;
    use std::collections::HashMap;

    struct FakeMachine {
        free: u64,
        fs: String,
        pauses: u32,
    }
    impl Machine for FakeMachine {
        fn stick_facts(&self) -> StickFacts {
            StickFacts { drive: "E:".into(), kind: DriveKind::Removable, label: "UPGV0".into(), file_system: self.fs.clone(), system_drive: "C:".into() }
        }
        fn free_bytes(&self, _: &Path) -> Result<u64, String> {
            Ok(self.free)
        }
        fn pause(&mut self, _: u64) {
            self.pauses += 1;
        }
    }

    /// A made-up server. `drops`: cut the body after this many bytes, once per entry.
    #[derive(Default)]
    struct FakeNet {
        files: HashMap<String, Vec<u8>>,
        drops: Vec<u64>,
        ignore_range: bool,
        status: Option<u16>,
        no_length: bool,
        calls: Vec<(String, u64)>,
    }
    struct Cut {
        data: Vec<u8>,
        at: usize,
        cut: Option<usize>,
    }
    impl Read for Cut {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            if self.cut == Some(self.at) {
                return Err(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "the connection was reset"));
            }
            let end = self.cut.unwrap_or(self.data.len()).min(self.data.len()).min(self.at + b.len().min(3000));
            let n = end - self.at;
            b[..n].copy_from_slice(&self.data[self.at..end]);
            self.at = end;
            Ok(n)
        }
    }
    impl Net for FakeNet {
        fn get(&mut self, url: &str, from: u64) -> Result<Response, String> {
            self.calls.push((url.to_string(), from));
            let Some(d) = self.files.get(url).cloned() else { return Ok(Response { status: 404, length: None, range_start: None, body: Box::new(std::io::empty()) }) };
            if let Some(s) = self.status {
                return Ok(Response { status: s, length: None, range_start: None, body: Box::new(std::io::empty()) });
            }
            let (status, body, start) = if from > 0 && !self.ignore_range { (206, d[from as usize..].to_vec(), Some(from)) } else { (200, d.clone(), None) };
            let cut = if self.calls.len() > 2 && !self.drops.is_empty() { Some(self.drops.remove(0) as usize).filter(|c| *c < body.len()) } else { None };
            let length = if self.no_length { None } else { Some(body.len() as u64) };
            Ok(Response { status, length, range_start: start, body: Box::new(Cut { data: body, at: 0, cut }) })
        }
    }

    fn hex(b: &[u8]) -> String {
        format!("{:x}", Sha256::digest(b))
    }

    struct Bench {
        root: PathBuf,
        plan: Plan,
        net: FakeNet,
        m: FakeMachine,
        lines: Vec<String>,
    }

    fn bench() -> Bench {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!("upg-fetch-get-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("upgrade_")).unwrap();
        std::fs::write(root.join("UPGRADE.exe"), "exe").unwrap();
        std::fs::write(root.join("KIT-MANIFEST.txt"), "kit").unwrap();
        std::fs::write(root.join("SHA256SUMS"), format!("{}  ./UPGRADE.exe\n", hex(b"exe"))).unwrap();
        let shim: Vec<u8> = (0..5000u32).map(|i| (i * 7 % 256) as u8).collect();
        let image: Vec<u8> = (0..20000u32).map(|i| (i * 13 % 256) as u8).collect();
        let net_iso = make_iso(&[("BOOTX64.EFI", &shim), ("vmlinuz", b"kernel")], &[], true, false);
        let live_iso = make_iso(&[], &[("squashfs.img", &image)], true, false);
        let plan = Plan {
            id: "fedora-44".into(),
            name: "Fedora 44".into(),
            desktop: "kde".into(),
            desktop_label: "KDE Plasma".into(),
            family: "fedora".into(),
            measured: "2026-10-03".into(),
            parts: vec![
                Part {
                    url: "https://example.org/a/net.iso".into(),
                    sha256: hex(&net_iso),
                    extracts: vec![
                        Extract { iso_path: "BOOTX64.EFI".into(), dest: "EFI\\BOOT\\BOOTX64.EFI".into(), sha256: Some(hex(&shim)) },
                        Extract { iso_path: "vmlinuz".into(), dest: "images\\pxeboot\\vmlinuz".into(), sha256: None },
                    ],
                },
                Part { url: "https://example.org/b/live.iso".into(), sha256: hex(&live_iso), extracts: vec![Extract { iso_path: "Sub/squashfs.img".into(), dest: "upgrade_\\LiveOS\\kde.squashfs".into(), sha256: Some(hex(&image)) }] },
            ],
            release_json: "{\n  \"id\": \"fedora-44\",\n  \"name\": \"Fedora 44\",\n  \"desktop\": \"kde\"\n}\n".into(),
            boot_chain: vec![("upgrade_\\boot-chain\\kde-shimx64.sbat".into(), "sbat,1\n".into())],
        };
        let mut net = FakeNet::default();
        net.files.insert("https://example.org/a/net.iso".into(), net_iso);
        net.files.insert("https://example.org/b/live.iso".into(), live_iso);
        Bench { root, plan, net, m: FakeMachine { free: 1 << 30, fs: "FAT32".into(), pauses: 0 }, lines: vec![] }
    }

    impl Bench {
        fn run(&mut self) -> Result<Outcome, String> {
            let lines = &mut self.lines;
            get(&self.root, &self.plan, None, &mut self.net, &mut self.m, &mut |l| lines.push(l.to_string()))
        }
        fn staging_gone(&self) -> bool {
            !self.root.join("upgrade_").join("fetch").exists()
        }
    }
    impl Drop for Bench {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn a_good_download_lands_whole_and_is_recorded_last() {
        let mut b = bench();
        assert!(matches!(b.run().unwrap(), Outcome::Fetched { .. }));
        let r = &b.root;
        assert!(stick::holds(r, &b.plan).is_ok());
        let sums = stick::read_sums(&std::fs::read_to_string(r.join("SHA256SUMS")).unwrap());
        let paths: Vec<&str> = sums.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, ["EFI\\BOOT\\BOOTX64.EFI", "UPGRADE.exe", "images\\pxeboot\\vmlinuz", "release.json", "upgrade_\\LiveOS\\kde.squashfs", "upgrade_\\boot-chain\\kde-shimx64.sbat"]);
        for (p, h) in &sums {
            assert_eq!(&sha256_file(&stick::under(r, p)).unwrap(), h, "{p} is what SHA256SUMS says");
        }
        assert_eq!(std::fs::read(r.join("images/pxeboot/vmlinuz")).unwrap(), b"kernel");
        assert!(b.staging_gone());
        assert!(b.lines.iter().any(|l| l.starts_with("fetch: progress ")));
        assert!(b.lines.last().unwrap().starts_with("fetch: ready"));
        // a second run downloads nothing
        let before = b.net.calls.len();
        assert_eq!(b.run().unwrap(), Outcome::AlreadyThere);
        assert_eq!(b.net.calls.len(), before);
    }

    #[test]
    fn a_dropped_connection_carries_on_from_where_it_stopped() {
        let mut b = bench();
        b.net.drops = vec![10_000, 30_000, 7];
        b.run().unwrap();
        assert!(stick::holds(&b.root, &b.plan).is_ok());
        assert!(b.net.calls.iter().any(|(_, from)| *from > 0), "a range request was made: {:?}", b.net.calls);
        assert_eq!(b.m.pauses, 3);
    }

    #[test]
    fn a_server_that_ignores_the_range_still_gives_the_right_bytes() {
        let mut b = bench();
        b.net.drops = vec![10_000];
        b.net.ignore_range = true;
        b.run().unwrap();
        assert!(stick::holds(&b.root, &b.plan).is_ok());
    }

    #[test]
    fn a_connection_that_never_holds_refuses_and_keeps_nothing() {
        let mut b = bench();
        b.net.drops = vec![100; 20];
        let e = b.run().unwrap_err();
        assert!(e.contains("kept failing"), "{e}");
        assert!(b.staging_gone());
        assert!(stick::holds(&b.root, &b.plan).is_err());
    }

    #[test]
    fn a_byte_that_differs_refuses_and_keeps_nothing() {
        let mut b = bench();
        let iso = b.net.files.get_mut("https://example.org/b/live.iso").unwrap();
        let n = iso.len();
        iso[n - 10] ^= 1;
        let e = b.run().unwrap_err();
        assert!(e.contains("did not match its measured sha256"), "{e}");
        assert!(b.staging_gone());
        assert!(!b.root.join("upgrade_/LiveOS/kde.squashfs").exists());
        assert!(!b.root.join("release.json").exists());
    }

    #[test]
    fn the_right_iso_with_a_file_the_table_did_not_measure_refuses() {
        let mut b = bench();
        b.plan.parts[0].extracts[0].sha256 = Some("0".repeat(64));
        assert!(b.run().unwrap_err().contains("is not the measured file"));
        assert!(b.staging_gone());
    }

    #[test]
    fn a_short_file_no_size_a_404_and_a_wrong_status_refuse() {
        let mut b = bench();
        let iso = b.net.files.get_mut("https://example.org/a/net.iso").unwrap();
        iso.truncate(iso.len() - 2048);
        assert!(b.run().unwrap_err().contains("did not match"));
        let mut b = bench();
        b.net.no_length = true;
        assert!(b.run().unwrap_err().contains("did not say how big"));
        let mut b = bench();
        b.net.files.remove("https://example.org/b/live.iso");
        assert!(b.run().unwrap_err().contains("answered 404"));
        let mut b = bench();
        b.net.status = Some(500);
        assert!(b.run().unwrap_err().contains("answered 500"));
    }

    #[test]
    fn no_room_refuses_before_anything_is_removed() {
        let mut b = bench();
        std::fs::create_dir_all(b.root.join("upgrade_/LiveOS")).unwrap();
        std::fs::write(b.root.join("upgrade_/LiveOS/gnome.squashfs"), "old").unwrap();
        b.m.free = 1000;
        assert!(b.run().unwrap_err().contains("use a bigger stick"));
        assert!(b.root.join("upgrade_/LiveOS/gnome.squashfs").exists(), "nothing removed");
    }

    #[test]
    fn another_release_on_the_stick_is_replaced_whole() {
        let mut b = bench();
        std::fs::create_dir_all(b.root.join("upgrade_/LiveOS")).unwrap();
        std::fs::write(b.root.join("upgrade_/LiveOS/gnome.squashfs"), "old").unwrap();
        std::fs::write(b.root.join("release.json"), "{\"id\":\"fedora-44\",\"desktop\":\"gnome\"}").unwrap();
        let old = std::fs::read_to_string(b.root.join("SHA256SUMS")).unwrap() + &format!("{}  ./upgrade_/LiveOS/gnome.squashfs\n", hex(b"old"));
        std::fs::write(b.root.join("SHA256SUMS"), old).unwrap();
        b.run().unwrap();
        assert!(!b.root.join("upgrade_/LiveOS/gnome.squashfs").exists());
        let sums = std::fs::read_to_string(b.root.join("SHA256SUMS")).unwrap();
        assert!(!sums.contains("gnome"));
    }

    #[test]
    fn content_range_start() {
        assert_eq!(range_start("bytes 1000-4999/5000"), Some(1000));
        assert_eq!(range_start("bytes */5000"), None);
    }

    #[test]
    fn rebase_keeps_the_file_name() {
        assert_eq!(rebase("https://dl.fedoraproject.org/pub/x/F.iso", Some("http://10.0.0.1:8000/")), "http://10.0.0.1:8000/F.iso");
        assert_eq!(rebase("https://a/b.iso", None), "https://a/b.iso");
    }
}
