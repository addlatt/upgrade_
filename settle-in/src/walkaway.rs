//! "Go back to Windows", 100% managed (decided 2026-09-29, the owner; RISKS
//! R33; architecture.md, "The way back to Windows", stage 2).
//!
//! Everything here happens in Linux, before the restart, and changes
//! nothing on this computer's drives. In order:
//!
//!   catalog   Microsoft's own catalog (the one its Media Creation Tool
//!             reads), over HTTPS: which file, its size and its SHA-1;
//!   download  that file (Microsoft's host refuses HTTPS, 2026-09-29), kept
//!             only if its size and SHA-1 match the catalog;
//!   tree      the stick's files built from it with wimlib: Setup's files,
//!             boot.wim with the gate inside and started first, and the one
//!             edition this computer had as install.wim;
//!   drives    the drives to erase, by serial, world-wide name and exact
//!             size: the one holding `/`, and the one holding `/home`;
//!   job       upgrade_/go-back.json for the gate, which checks it all again.
//!
//! The stick is written by stickwrite (R16's rules). The commit line is the
//! gate's countdown, on the stick, after the restart (settle-in/gate).

use serde_json::{json, Value};
use std::process::Command;

pub const CATALOG_11: &str = "https://go.microsoft.com/fwlink/?linkid=2156292";
pub const CATALOG_10: &str = "https://go.microsoft.com/fwlink/?LinkId=841361";
pub const SENTENCE: &str = "I confirm that Linux and everything on this computer will be deleted and nothing will be kept";
const PATH_ENV: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

fn run(prog: &str, args: &[&str]) -> Result<String, String> {
    let o = Command::new(prog).args(args).env("PATH", PATH_ENV).env("LC_ALL", "C").output().map_err(|e| format!("{} could not start ({})", prog, e))?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).to_string())
    } else {
        Err(format!("{} failed: {}", prog, String::from_utf8_lossy(&o.stderr).trim()))
    }
}

// ---------------------------------------------------------------- the catalog

/// One Windows file from the catalog. Pure.
pub fn pick(xml: &str, language: &str, edition: &str) -> Result<Value, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| format!("the catalog could not be read ({})", e))?;
    let text = |n: &roxmltree::Node, k: &str| n.children().find(|c| c.has_tag_name(k)).and_then(|c| c.text()).unwrap_or("").trim().to_string();
    for f in doc.descendants().filter(|n| n.has_tag_name("File")) {
        if text(&f, "Architecture") == "x64" && text(&f, "LanguageCode").eq_ignore_ascii_case(language) && text(&f, "Edition") == edition {
            let url = text(&f, "FilePath");
            let size: u64 = text(&f, "Size").parse().unwrap_or(0);
            let sha1 = text(&f, "Sha1").to_ascii_lowercase();
            if !url.starts_with("http://dl.delivery.mp.microsoft.com/") && !url.starts_with("https://dl.delivery.mp.microsoft.com/") {
                return Err(format!("the catalog points outside Microsoft's download host ({})", url));
            }
            if size == 0 || sha1.len() != 40 || !sha1.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err("the catalog's entry has no size or checksum".into());
            }
            return Ok(json!({ "file": text(&f, "FileName"), "url": url, "size": size, "sha1": sha1, "edition": edition, "language": text(&f, "LanguageCode") }));
        }
    }
    Err(format!("Microsoft's catalog has no {} file for {}", edition, language))
}

/// The catalog's edition name for what this computer had. Pure.
/// Home and Pro are in the consumer file; anything else falls back to Home.
pub fn catalog_edition(before: &Value) -> &'static str {
    match before["edition_id"].as_str().unwrap_or("") {
        "Professional" => "Professional",
        "ProfessionalN" => "ProfessionalN",
        "CoreN" => "CoreN",
        "CoreSingleLanguage" => "CoreSingleLanguage",
        "Education" => "Education",
        _ => "Core",
    }
}

/// Fetch Microsoft's catalog over HTTPS and unpack it with cabextract
/// (static, carried with settle-in: the cabinet is LZX-compressed, 2026-09-29).
pub fn fetch_catalog(windows: &str, dir: &str, cabextract: &str) -> Result<String, String> {
    let url = if windows == "10" { CATALOG_10 } else { CATALOG_11 };
    let cab = format!("{}/products-win{}.cab", dir, windows);
    // HTTPS only, redirects allowed only to HTTPS (go.microsoft.com -> download.microsoft.com)
    run("curl", &["--fail", "--silent", "--show-error", "--location", "--proto", "=https", "--proto-redir", "=https", "--max-time", "120", "-o", &cab, url])?;
    unpack_catalog(&cab, dir, cabextract)
}

/// products.xml out of the cabinet; cabextract checks the cabinet's own checksums.
pub fn unpack_catalog(cab: &str, dir: &str, cabextract: &str) -> Result<String, String> {
    let out = format!("{}/catalog", dir);
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {}", out, e))?;
    run(cabextract, &["-q", "-d", &out, "-F", "products.xml", cab])?;
    std::fs::read_to_string(format!("{}/products.xml", out)).map_err(|_| "the catalog did not contain products.xml".to_string())
}

/// SHA-1 of a file (coreutils' sha1sum: every Linux has it).
pub fn sha1_file(path: &str) -> Result<String, String> {
    Ok(run("sha1sum", &[path])?.split_whitespace().next().unwrap_or("").to_ascii_lowercase())
}

/// Download (resuming a partial file) and keep it only if it is the catalog's.
pub fn download(entry: &Value, dir: &str) -> Result<String, String> {
    let name = entry["file"].as_str().unwrap_or("windows.esd");
    if name.contains('/') || name.starts_with('.') {
        return Err("the catalog's file name is not a plain name".into());
    }
    let (dst, part) = (format!("{}/{}", dir, name), format!("{}/{}.part", dir, name));
    let size = entry["size"].as_u64().unwrap_or(0);
    let ok = |p: &str| std::fs::metadata(p).map(|m| m.len() == size).unwrap_or(false) && sha1_file(p).ok().as_deref() == entry["sha1"].as_str();
    if ok(&dst) {
        return Ok(dst);
    }
    let url = entry["url"].as_str().unwrap_or("");
    run("curl", &["--fail", "--silent", "--show-error", "--location", "--proto", "=http,https", "--retry", "5", "-C", "-", "-o", &part, url])?;
    if std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0) != size {
        return Err("the download is not the size Microsoft's catalog says".into());
    }
    if sha1_file(&part)?.as_str() != entry["sha1"].as_str().unwrap_or("") {
        let _ = std::fs::remove_file(&part);
        return Err("the download is not the file Microsoft's catalog names (its checksum differs); it was deleted".into());
    }
    std::fs::rename(&part, &dst).map_err(|e| format!("{}: {}", dst, e))?;
    Ok(dst)
}

// ---------------------------------------------------------------- the stick's files

/// The image index of an edition inside the file, from `wimlib-imagex info`. Pure.
pub fn edition_index(info: &str, edition: &str) -> Option<u32> {
    let mut idx: Option<u32> = None;
    for l in info.lines() {
        if let Some(v) = l.strip_prefix("Index:") {
            idx = v.trim().parse().ok();
        } else if let Some(v) = l.strip_prefix("Edition ID:") {
            if v.trim() == edition {
                return idx;
            }
        }
    }
    None
}

/// SetupComplete.cmd: runs once when Windows is installed. No password rides
/// on the stick: the account starts empty and must be changed at first
/// sign-in (rig spike, 2026-09-29). Pure.
pub fn setup_complete(account: &str) -> String {
    format!(
        "@echo off\r\nrem written by settle-in {}: the account has no password yet; Windows asks for a new one at the first sign-in\r\nnet user \"{}\" /logonpasswordchg:yes > \"%WINDIR%\\Setup\\Scripts\\upgrade_-setupcomplete.log\" 2>&1\r\nfor %%d in (C D E F G H I J K L M N O P Q R S T U V W Y Z) do if exist %%d:\\upgrade_\\go-back-gate.json (echo installed %DATE% %TIME%> %%d:\\upgrade_\\go-back-installed.txt)\r\n",
        crate::VERSION, account
    )
}

pub const WINPESHL: &str = "[LaunchApps]\r\n%SYSTEMROOT%\\System32\\wpeinit.exe\r\n%SYSTEMROOT%\\System32\\upgrade-gate.exe\r\n";

/// Build the stick's files in `tree` (a new folder) from the .esd.
pub fn build_tree(esd: &str, edition: &str, account: &str, tree: &str, gate: &str, wimlib: &str) -> Result<Value, String> {
    if std::path::Path::new(tree).exists() {
        return Err(format!("{} already exists", tree));
    }
    if !std::path::Path::new(gate).exists() {
        return Err(format!("the gate program is missing ({})", gate));
    }
    let info = run(wimlib, &["info", esd])?;
    let index = edition_index(&info, edition).ok_or_else(|| format!("the Windows file has no {} edition", edition))?;
    if edition_index(&info, "WindowsPE").is_none() || !info.contains("Index:                  3") {
        return Err("the Windows file is not laid out as Microsoft's setup file (Setup, WinPE, Setup's WinPE, editions)".into());
    }
    std::fs::create_dir_all(tree).map_err(|e| format!("{}: {}", tree, e))?;
    run(wimlib, &["apply", esd, "1", tree])?;
    let boot = format!("{}/sources/boot.wim", tree);
    run(wimlib, &["export", esd, "2", &boot, "--compress=LZX"])?;
    run(wimlib, &["export", esd, "3", &boot, "--boot", "--compress=LZX"])?;
    let ini = format!("{}.winpeshl.ini", tree);
    std::fs::write(&ini, WINPESHL).map_err(|e| e.to_string())?;
    run(wimlib, &["update", &boot, "2", &format!("--command=add {} /Windows/System32/upgrade-gate.exe", gate)])?;
    run(wimlib, &["update", &boot, "2", &format!("--command=add {} /Windows/System32/winpeshl.ini", ini)])?;
    let _ = std::fs::remove_file(&ini);
    run(wimlib, &["export", esd, &index.to_string(), &format!("{}/sources/install.wim", tree), "--compress=LZX"])?;
    let scripts = format!("{}/sources/$OEM$/$$/Setup/Scripts", tree);
    std::fs::create_dir_all(&scripts).map_err(|e| e.to_string())?;
    std::fs::write(format!("{}/SetupComplete.cmd", scripts), setup_complete(account)).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(format!("{}/upgrade_", tree)).map_err(|e| e.to_string())?;
    Ok(json!({ "edition": edition, "esd_index": index }))
}

/// A world-wide name only if it is one: "0x" / "naa." / "eui." and 16+ hex
/// digits. Linux falls back to "t10.ATA <model> <serial>" text for SATA drives
/// without one (the Aspire's SSD, 2026-09-29): that is not an identity. Pure.
pub fn clean_wwn(s: &str) -> String {
    let t = s.trim().to_ascii_lowercase();
    let body = t.strip_prefix("0x").or_else(|| t.strip_prefix("naa.")).or_else(|| t.strip_prefix("eui."));
    match body {
        Some(b) if b.len() >= 16 && b.chars().all(|c| c.is_ascii_hexdigit()) => t,
        _ => String::new(),
    }
}

/// Free bytes where the files are prepared.
pub fn free_bytes(dir: &str) -> u64 {
    let c = std::ffi::CString::new(dir).unwrap_or_default();
    // SAFETY: statvfs fills the struct it is given; c is a valid C string.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return 0;
    }
    st.f_bavail as u64 * st.f_frsize as u64
}

/// The same rule the gate applies (settle-in/gate, logic::account_ok). Pure.
pub fn account_ok(name: &str) -> bool {
    let reserved = ["administrator", "guest", "defaultaccount", "wdagutilityaccount", "system", "none"];
    !name.is_empty()
        && name.len() <= 20
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
        && !name.ends_with('.')
        && !reserved.contains(&name.to_ascii_lowercase().as_str())
}

// ---------------------------------------------------------------- the drives

fn read(p: &str) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

/// The whole disks under a block device (major:minor), through partitions,
/// device-mapper and RAID layers. Pure over a sysfs tree at `root`.
pub fn disks_under(root: &str, devnum: &str) -> Vec<String> {
    let r = root.trim_end_matches('/');
    let Ok(p) = std::fs::canonicalize(format!("{}/sys/dev/block/{}", r, devnum)) else { return vec![] };
    let mut out = Vec::new();
    let mut stack = vec![p];
    while let Some(p) = stack.pop() {
        let slaves: Vec<_> = std::fs::read_dir(p.join("slaves")).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
        if !slaves.is_empty() {
            for s in slaves {
                if let Ok(c) = std::fs::canonicalize(&s) {
                    stack.push(c);
                }
            }
        } else if p.join("partition").exists() {
            if let Some(parent) = p.parent().and_then(|x| x.file_name()) {
                out.push(parent.to_string_lossy().to_string());
            }
        } else if let Some(n) = p.file_name() {
            out.push(n.to_string_lossy().to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The drives to erase: the one holding `/`, then the one holding `/home`
/// if it is another. Refuses layouts it cannot name exactly. Pure over `root`.
pub fn drives(root: &str, disks: &Value) -> Result<Vec<Value>, String> {
    let r = root.trim_end_matches('/');
    let mi = read(&format!("{}/proc/self/mountinfo", r)).unwrap_or_default();
    let devnum_of = |mp: &str| mi.lines().find_map(|l| {
        let f: Vec<&str> = l.split(' ').collect();
        (f.get(4) == Some(&mp)).then(|| f.get(2).map(|s| s.to_string())).flatten()
    });
    let root_dev = devnum_of("/").ok_or("what holds / could not be found")?;
    let sys = disks_under(r, &root_dev);
    let home = devnum_of("/home").map(|d| disks_under(r, &d)).unwrap_or_default();
    if sys.len() != 1 {
        return Err(format!("Linux here is spread over {} drives; this program only erases a system on one drive", sys.len()));
    }
    let mut names = vec![("system", sys[0].clone())];
    match home.as_slice() {
        [] => {}
        [h] if *h == sys[0] => {}
        [h] => names.push(("second", h.clone())),
        _ => return Err("/home is spread over several drives; this program does not erase that".into()),
    }
    let mut out = Vec::new();
    for (role, n) in names {
        let d = disks.as_array().into_iter().flatten().find(|d| d["name"] == json!(n)).ok_or_else(|| format!("{} is not in the disk list", n))?;
        if d["usb"] == json!(true) || d["removable"] == json!(true) {
            return Err(format!("the {} drive ({}) is a USB or removable drive; Linux is not installed on this computer's own drive", role, n));
        }
        let serial = d["serial"].as_str().unwrap_or("").to_string();
        let wwn = clean_wwn(d["wwn"].as_str().unwrap_or(""));
        if serial.trim().is_empty() && wwn.trim().is_empty() {
            return Err(format!("the {} drive ({}) reports no serial number or world-wide name, so it could not be found again safely", role, n));
        }
        out.push(json!({ "role": role, "name": n, "serial": serial, "wwn": wwn, "size_bytes": d["size_bytes"], "model": d["model"] }));
    }
    Ok(out)
}

/// The gate's job. Pure.
pub fn job(job_id: &str, now: &str, typed: &str, windows: &str, edition: &str, edition_name: &str, language: &str, account: &str, drives: &[Value], entry: &Value) -> Value {
    json!({
        "schema": "go-back-job/1",
        "job_id": job_id,
        "created_utc": now,
        "settle_in_version": crate::VERSION,
        "consent": { "sentence": typed, "typed_utc": now },
        "windows": { "version": windows, "edition": edition, "edition_name": edition_name, "image_index": 1, "language": language,
                     "source": { "file": entry["file"], "sha1": entry["sha1"], "size": entry["size"] } },
        "account": { "name": account },
        "drives": drives.iter().map(|d| json!({ "role": d["role"], "serial": d["serial"], "wwn": d["wwn"], "size_bytes": d["size_bytes"], "model": d["model"] })).collect::<Vec<_>>(),
    })
}

/// A one-time boot of the stick: a new firmware entry that is NOT added to the
/// boot order (efibootmgr -C), then BootNext. Linux starts again after it.
pub fn boot_once(stick_disk: &str) -> Result<Value, String> {
    let dev = format!("/dev/{}", stick_disk);
    let out = run("efibootmgr", &["-C", "-d", &dev, "-p", "1", "-L", "upgrade_ go back to Windows", "-l", "\\EFI\\BOOT\\BOOTX64.EFI"])?;
    let num = out.lines().filter_map(|l| l.strip_prefix("Boot")).filter(|l| l.contains("upgrade_ go back to Windows"))
        .filter_map(|l| l.get(0..4)).last().ok_or("the new firmware entry could not be found")?.to_string();
    run("efibootmgr", &["-n", &num])?;
    Ok(json!({ "entry": format!("Boot{}", num), "boot_next": num }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<MCT><Catalogs><Catalog><PublishedMedia><Files>
      <File id="1"><FileName>26100.x_en-us.esd</FileName><LanguageCode>en-us</LanguageCode><Language>English (United States)</Language><Edition>Core</Edition><Architecture>x64</Architecture><Size>4680320173</Size><Sha1>8ceab2838f8e90180ac7490e8752157fb05588cb</Sha1><FilePath>http://dl.delivery.mp.microsoft.com/filestreamingservice/files/009d9a0d/26100.x_en-us.esd</FilePath></File>
      <File id="2"><FileName>26100.x_fr-fr.esd</FileName><LanguageCode>fr-fr</LanguageCode><Edition>Core</Edition><Architecture>x64</Architecture><Size>1</Size><Sha1>aa</Sha1><FilePath>http://dl.delivery.mp.microsoft.com/x</FilePath></File>
      <File id="3"><FileName>evil.esd</FileName><LanguageCode>de-de</LanguageCode><Edition>Core</Edition><Architecture>x64</Architecture><Size>1</Size><Sha1>8ceab2838f8e90180ac7490e8752157fb05588cb</Sha1><FilePath>http://example.com/evil.esd</FilePath></File>
    </Files></PublishedMedia></Catalog></Catalogs></MCT>"#;

    #[test]
    fn reads_microsofts_real_catalogs_when_present() {
        // the two catalogs read 2026-09-29 (rig artifacts, gitignored) through the
        // static cabextract settle-in carries (tools/build-cabextract.sh)
        let base = env!("CARGO_MANIFEST_DIR");
        let cx = format!("{}/target/cabextract/cabextract", base);
        if !std::path::Path::new(&cx).exists() {
            return;
        }
        for (f, n) in [("products-win11.cab", "26100"), ("products-win10.cab", "19045")] {
            let cab = format!("{}/../rig/vm/artifacts/windows/{}", base, f);
            if !std::path::Path::new(&cab).exists() {
                continue;
            }
            let dir = std::env::temp_dir().join(format!("walkaway-cat-{}-{}", n, std::process::id()));
            let x = unpack_catalog(&cab, dir.to_str().unwrap(), &cx).unwrap();
            let e = pick(&x, "en-us", "Core").unwrap();
            assert!(e["file"].as_str().unwrap().starts_with(n), "{}", e);
            assert_eq!(e["sha1"].as_str().unwrap().len(), 40);
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }

    #[test]
    fn picks_the_language_and_refuses_bad_entries() {
        let e = pick(XML, "EN-US", "Core").unwrap();
        assert_eq!(e["size"], 4680320173u64);
        assert_eq!(e["sha1"], "8ceab2838f8e90180ac7490e8752157fb05588cb");
        assert!(pick(XML, "fr-fr", "Core").unwrap_err().contains("no size or checksum"));
        assert!(pick(XML, "de-de", "Core").unwrap_err().contains("outside Microsoft's download host"));
        assert!(pick(XML, "en-us", "Professional").is_err());
    }

    #[test]
    fn editions_and_their_index() {
        assert_eq!(catalog_edition(&json!({ "edition_id": "Professional" })), "Professional");
        assert_eq!(catalog_edition(&json!({ "edition_id": "Enterprise" })), "Core");
        assert_eq!(catalog_edition(&json!({})), "Core");
        let info = "Index:                  3\nName: Setup\nEdition ID:             WindowsPE\nIndex:                  4\nEdition ID:             Core\nIndex:                  9\nEdition ID:             Professional\n";
        assert_eq!(edition_index(info, "Core"), Some(4));
        assert_eq!(edition_index(info, "Professional"), Some(9));
        assert_eq!(edition_index(info, "Education"), None);
    }

    #[test]
    fn setup_complete_carries_no_password() {
        let s = setup_complete("rig");
        assert!(s.contains("net user \"rig\" /logonpasswordchg:yes"));
        assert!(!s.to_lowercase().contains("password:"));
    }

    fn fake_sys(tag: &str) -> std::path::PathBuf {
        // sda (system: sda2 is /), sdb (home: its partition under an LVM volume is /home), sdc (USB stick)
        let r = std::env::temp_dir().join(format!("walkaway-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&r);
        let mk = |p: &str| std::fs::create_dir_all(r.join(p)).unwrap();
        let ln = |target: &str, link: &str| std::os::unix::fs::symlink(r.join(target), r.join(link)).unwrap();
        mk("sys/devices/pci/sda/sda2");
        std::fs::write(r.join("sys/devices/pci/sda/sda2/partition"), "2").unwrap();
        mk("sys/devices/pci/sdb/sdb1");
        std::fs::write(r.join("sys/devices/pci/sdb/sdb1/partition"), "1").unwrap();
        mk("sys/devices/virtual/block/dm-0/slaves");
        ln("sys/devices/pci/sdb/sdb1", "sys/devices/virtual/block/dm-0/slaves/sdb1");
        mk("sys/dev/block");
        ln("sys/devices/pci/sda/sda2", "sys/dev/block/8:2");
        ln("sys/devices/virtual/block/dm-0", "sys/dev/block/253:0");
        mk("proc/self");
        std::fs::write(r.join("proc/self/mountinfo"), "1 0 8:2 / / rw - btrfs /dev/sda2 rw\n2 1 253:0 / /home rw - ext4 /dev/mapper/v-home rw\n").unwrap();
        r
    }

    fn disks() -> Value {
        json!([
            { "name": "sda", "size_bytes": 256060514304u64, "usb": false, "removable": false, "serial": "EI8AN00951150A71I", "wwn": "", "model": "HFS256G39TND-N210A" },
            { "name": "sdb", "size_bytes": 1000204886016u64, "usb": false, "removable": false, "serial": "WD-WXV1A88H0TFV", "wwn": "0x50014ee2b5a8e5a1", "model": "WDC WD10SPZX-21Z10T0" },
            { "name": "sdc", "size_bytes": 15664676864u64, "usb": true, "removable": true, "serial": "", "wwn": "", "model": "General UDisk" }
        ])
    }

    #[test]
    fn names_the_system_drive_and_the_home_drive_through_lvm() {
        let r = fake_sys("two");
        assert_eq!(disks_under(r.to_str().unwrap(), "253:0"), vec!["sdb".to_string()]);
        let d = drives(r.to_str().unwrap(), &disks()).unwrap();
        assert_eq!((d[0]["role"].as_str(), d[0]["name"].as_str()), (Some("system"), Some("sda")));
        assert_eq!((d[1]["role"].as_str(), d[1]["name"].as_str()), (Some("second"), Some("sdb")));
        assert_eq!(d[1]["wwn"], "0x50014ee2b5a8e5a1");
        std::fs::remove_dir_all(&r).unwrap();
    }

    #[test]
    fn refuses_a_system_on_usb_and_a_drive_with_no_identity() {
        let r = fake_sys("usb");
        let mut ds = disks();
        ds[0]["usb"] = json!(true);
        assert!(drives(r.to_str().unwrap(), &ds).unwrap_err().contains("USB or removable"));
        let mut ds = disks();
        ds[0]["serial"] = json!("");
        assert!(drives(r.to_str().unwrap(), &ds).unwrap_err().contains("no serial number or world-wide name"));
        std::fs::remove_dir_all(&r).unwrap();
    }

    #[test]
    fn only_real_world_wide_names_count() {
        assert_eq!(clean_wwn("0x50014EE6B3A6DB94"), "0x50014ee6b3a6db94");
        assert_eq!(clean_wwn("eui.000000000000000100a07524480c575b"), "eui.000000000000000100a07524480c575b");
        assert_eq!(clean_wwn("t10.ATA     HFS256G39TND-N210A                      EI8AN00951150A71I"), "");
        assert_eq!(clean_wwn("0x1234"), "");
    }

    #[test]
    fn the_job_is_what_the_gate_reads() {
        let d = vec![json!({ "role": "system", "name": "sda", "serial": "S", "wwn": "", "size_bytes": 1u64, "model": "M" })];
        let j = job("id", "2026-09-29T00:00:00Z", SENTENCE, "11", "Core", "Windows 11 Home", "en-US", "rig", &d, &json!({ "file": "f.esd", "sha1": "ab", "size": 2 }));
        assert_eq!(j["schema"], "go-back-job/1");
        assert_eq!(j["consent"]["sentence"], SENTENCE);
        assert_eq!(j["windows"]["image_index"], 1);
        assert!(j["drives"][0].get("name").is_none(), "Linux's own disk names mean nothing to the gate");
    }
}
