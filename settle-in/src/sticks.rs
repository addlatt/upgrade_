//! Which USB stick may become a Windows installer: R16's refusals, on Linux
//! (RISKS R16, R30). Read-only.
//!
//! `collect` reads every block disk from sysfs, the udev database and the
//! mount table into plain JSON facts, so a real machine's list can be saved
//! and replayed in tests forever (rule #5). `judge` is a pure function over
//! those facts: every disk comes back with the rules it breaks, and only a
//! disk that breaks none is offered. `choose` is the one the writer uses:
//! the person's pick is found again by serial number from a fresh list, its
//! size must match to the byte, and the word they typed must be the one they
//! were shown. Disk names (sdb, sdc) are never accepted: they move.

use serde_json::{json, Value};

const DESKTOP_MEDIA: [&str; 2] = ["/run/media/", "/media/"];
const SYSTEM_MOUNTS: [&str; 7] = ["/", "/boot", "/boot/efi", "/efi", "/home", "/usr", "/var"];

fn read(p: &str) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

/// udev's properties for a block device (E:KEY=VALUE lines), by major:minor.
fn udev(root: &str, devnum: &str) -> std::collections::BTreeMap<String, String> {
    let mut m = std::collections::BTreeMap::new();
    if let Some(t) = read(&format!("{}/run/udev/data/b{}", root, devnum)) {
        for l in t.lines() {
            if let Some((k, v)) = l.strip_prefix("E:").and_then(|kv| kv.split_once('=')) {
                m.insert(k.to_string(), v.to_string());
            }
        }
    }
    m
}

/// Mount points by major:minor, from the kernel's own table.
fn mounts(root: &str) -> Vec<(String, String)> {
    read(&format!("{}/proc/self/mountinfo", root)).unwrap_or_default().lines().filter_map(|l| {
        let f: Vec<&str> = l.split(' ').collect();
        Some((f.get(2)?.to_string(), f.get(4)?.replace("\\040", " ")))
    }).collect()
}

/// Facts about every whole disk. No judgement here.
pub fn collect(root: &str) -> Value {
    let r = root.trim_end_matches('/');
    let mounts = mounts(r);
    let swaps = read(&format!("{}/proc/swaps", r)).unwrap_or_default();
    let mut disks = Vec::new();
    let Ok(rd) = std::fs::read_dir(format!("{}/sys/block", r)) else { return json!([]) };
    let mut names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    names.sort();
    for name in names {
        if ["loop", "ram", "zram", "dm-", "md", "sr", "nbd"].iter().any(|p| name.starts_with(p)) {
            continue;
        }
        let sys = format!("{}/sys/block/{}", r, name);
        let link = std::fs::read_link(&sys).map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        let devnum = read(&format!("{}/dev", sys)).unwrap_or_default();
        let u = udev(r, &devnum);
        let mut parts = Vec::new();
        let mut all = vec![(name.clone(), devnum.clone(), sys.clone())];
        if let Ok(rd) = std::fs::read_dir(&sys) {
            let mut pn: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.starts_with(&name) && std::path::Path::new(&format!("{}/{}/partition", sys, n)).exists()).collect();
            pn.sort();
            for p in pn {
                let d = read(&format!("{}/{}/dev", sys, p)).unwrap_or_default();
                all.push((p.clone(), d, format!("{}/{}", sys, p)));
            }
        }
        for (i, (pname, d, psys)) in all.iter().enumerate() {
            let pu = udev(r, d);
            let holders = std::fs::read_dir(format!("{}/holders", psys)).map(|h| h.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect::<Vec<_>>()).unwrap_or_default();
            let mnt: Vec<&String> = mounts.iter().filter(|(dn, _)| dn == d).map(|(_, m)| m).collect();
            let entry = json!({
                "name": pname,
                "label": pu.get("ID_FS_LABEL"),
                "fs": pu.get("ID_FS_TYPE"),
                "mounted_at": mnt,
                "swap": swaps.lines().any(|l| l.starts_with(&format!("/dev/{} ", pname))),
                "holders": holders,
            });
            if i == 0 {
                // the whole disk itself may carry a file system or a holder
                if !mnt.is_empty() || !entry["holders"].as_array().unwrap().is_empty() || entry["swap"] == json!(true) || pu.contains_key("ID_FS_TYPE") {
                    parts.push(entry);
                }
            } else {
                parts.push(entry);
            }
        }
        let sectors = read(&format!("{}/size", sys)).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
        disks.push(json!({
            "name": name,
            "devnum": devnum,
            "size_bytes": sectors * 512,
            "usb": link.contains("/usb"),
            "bus": u.get("ID_BUS"),
            "removable": read(&format!("{}/removable", sys)) == Some("1".into()),
            "rotational": read(&format!("{}/queue/rotational", sys)) == Some("1".into()),
            "read_only": read(&format!("{}/ro", sys)) == Some("1".into()),
            "vendor": read(&format!("{}/device/vendor", sys)),
            "model": read(&format!("{}/device/model", sys)).or_else(|| u.get("ID_MODEL").cloned()),
            "serial": u.get("ID_SERIAL_SHORT").or(u.get("ID_SERIAL")),
            // the world-wide name (NAA / EUI): Hyper-V's disks have no serial in WinPE,
            // and Windows gives NVMe drives' EUI as their serial (R33, 2026-09-29)
            "wwn": u.get("ID_WWN_WITH_EXTENSION").or(u.get("ID_WWN")).cloned().or_else(|| read(&format!("{}/wwid", sys)).or_else(|| read(&format!("{}/device/wwid", sys)))),
            "partitions": parts,
        }));
    }
    Value::Array(disks)
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

/// Every rule this disk breaks, in plain words. Empty = may be offered. Pure.
pub fn refusals(d: &Value, all: &[Value], min_bytes: u64) -> Vec<String> {
    let mut why = Vec::new();
    if d["usb"] != json!(true) {
        why.push("it is not a USB device".to_string());
    }
    if d["removable"] != json!(true) {
        why.push("it does not call itself removable (USB hard drives and SSDs usually don't; they are somebody's backup)".to_string());
    }
    if d["rotational"] == json!(true) {
        why.push("it is a hard drive, not a stick".to_string());
    }
    if d["read_only"] == json!(true) {
        why.push("it is read-only".to_string());
    }
    let size = d["size_bytes"].as_u64().unwrap_or(0);
    if size < min_bytes {
        why.push(format!("it is too small ({:.1} GB; the installer needs {:.1} GB)", size as f64 / 1e9, min_bytes as f64 / 1e9));
    }
    let serial = s(d, "serial");
    if serial.is_empty() {
        why.push("it has no serial number, so it cannot be found again for certain".to_string());
    } else if all.iter().filter(|o| s(o, "serial") == serial).count() > 1 {
        why.push("another disk has the same serial number, so which is which is not certain".to_string());
    }
    for p in d["partitions"].as_array().cloned().unwrap_or_default() {
        let pn = s(&p, "name");
        for m in p["mounted_at"].as_array().cloned().unwrap_or_default() {
            let m = m.as_str().unwrap_or("");
            if SYSTEM_MOUNTS.contains(&m) {
                why.push(format!("it holds part of the running system ({} is {})", pn, m));
            } else if !DESKTOP_MEDIA.iter().any(|dm| m.starts_with(dm)) {
                why.push(format!("{} is in use at {}", pn, m));
            }
        }
        if p["swap"] == json!(true) {
            why.push(format!("{} is this system's swap space", pn));
        }
        if !p["holders"].as_array().map(|h| h.is_empty()).unwrap_or(true) {
            why.push(format!("{} is used by the system (encryption, LVM or RAID)", pn));
        }
        let label = s(&p, "label").to_ascii_uppercase();
        if label.starts_with("UPG") {
            why.push(format!("it is the upgrade_ stick ({} is labelled {}); keep it", pn, s(&p, "label")));
        }
    }
    why.dedup();
    why
}

/// The word the person types to name the stick: the model as the stick
/// reports it (a label changes when the stick is formatted; the model does
/// not). Never "yes".
pub fn confirm_word(d: &Value) -> String {
    let m = s(d, "model").trim().to_string();
    if m.is_empty() { s(d, "vendor").trim().to_string() } else { m }
}

/// The list the person is shown. Pure.
pub fn judge(disks: &Value, min_bytes: u64) -> Value {
    let all = disks.as_array().cloned().unwrap_or_default();
    Value::Array(all.iter().map(|d| {
        let why = refusals(d, &all, min_bytes);
        json!({
            "name": d["name"], "serial": d["serial"], "size_bytes": d["size_bytes"],
            "shown_as": format!("{} {} ({:.1} GB)", s(d, "vendor"), s(d, "model"), d["size_bytes"].as_u64().unwrap_or(0) as f64 / 1e9).trim().to_string(),
            "confirm_word": confirm_word(d),
            "offered": why.is_empty(),
            "refused_because": why,
        })
    }).collect())
}

/// The writer's pick, from a FRESH collect: by serial, exact size, the typed
/// word. Any doubt is a refusal. Pure.
pub fn choose(disks: &Value, serial: &str, size_bytes: u64, typed: &str, min_bytes: u64) -> Result<Value, Vec<String>> {
    let all = disks.as_array().cloned().unwrap_or_default();
    let hits: Vec<&Value> = all.iter().filter(|d| !serial.is_empty() && s(d, "serial") == serial).collect();
    let d = match hits.len() {
        0 => return Err(vec!["that stick is not plugged in any more".to_string()]),
        1 => hits[0],
        _ => return Err(vec!["more than one disk has that serial number".to_string()]),
    };
    let mut why = refusals(d, &all, min_bytes);
    if d["size_bytes"].as_u64() != Some(size_bytes) {
        why.push(format!("its size is not the size you were shown ({} bytes now, {} shown)", d["size_bytes"], size_bytes));
    }
    let word = confirm_word(d);
    if word.is_empty() || typed.trim() != word {
        why.push(format!("the name typed does not match the stick (type {} exactly)", if word.is_empty() { "its name".to_string() } else { format!("\"{}\"", word) }));
    }
    if why.is_empty() { Ok(d.clone()) } else { Err(why) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1_000_000_000;

    fn stick(name: &str, serial: &str, size: u64) -> Value {
        json!({ "name": name, "size_bytes": size, "usb": true, "removable": true, "rotational": false, "read_only": false,
                "vendor": "SanDisk", "model": "Cruzer Blade", "serial": serial,
                "partitions": [{ "name": format!("{}1", name), "label": "MYSTICK", "fs": "vfat", "mounted_at": ["/run/media/pat/MYSTICK"], "swap": false, "holders": [] }] })
    }

    fn system() -> Value {
        json!({ "name": "sda", "size_bytes": 256 * GB, "usb": false, "removable": false, "rotational": false, "read_only": false,
                "vendor": "ATA", "model": "SK hynix SC311", "serial": "EI8AN0001",
                "partitions": [{ "name": "sda1", "mounted_at": ["/boot/efi"], "swap": false, "holders": [] },
                               { "name": "sda3", "mounted_at": ["/"], "swap": false, "holders": [] }] })
    }

    fn one(d: &Value) -> Vec<String> {
        refusals(d, std::slice::from_ref(d), 8 * GB)
    }

    #[test]
    fn a_plain_stick_is_offered() {
        assert!(one(&stick("sdc", "4C530001", 16 * GB)).is_empty());
    }

    #[test]
    fn the_system_disk_is_refused_many_ways() {
        let w = one(&system());
        assert!(w.iter().any(|x| x.contains("not a USB")));
        assert!(w.iter().any(|x| x.contains("running system (sda3 is /)")));
        assert!(w.iter().any(|x| x.contains("sda1 is /boot/efi")));
    }

    #[test]
    fn a_usb_ssd_or_hard_drive_is_refused() {
        let mut d = stick("sdd", "S1", 1000 * GB);
        d["removable"] = json!(false);
        assert!(one(&d).iter().any(|x| x.contains("somebody's backup")));
        d["rotational"] = json!(true);
        assert!(one(&d).iter().any(|x| x.contains("hard drive")));
    }

    #[test]
    fn the_upgrade_stick_is_refused() {
        let mut d = stick("sdc", "S1", 16 * GB);
        d["partitions"][0]["label"] = json!("UPGRADE");
        assert!(one(&d).iter().any(|x| x.contains("the upgrade_ stick")));
        d["partitions"][0]["label"] = json!("UPGDATA");
        assert!(!one(&d).is_empty());
    }

    #[test]
    fn too_small_read_only_no_serial_are_refused() {
        let mut d = stick("sdc", "", 4 * GB);
        d["read_only"] = json!(true);
        let w = one(&d);
        assert!(w.iter().any(|x| x.contains("too small")));
        assert!(w.iter().any(|x| x.contains("read-only")));
        assert!(w.iter().any(|x| x.contains("no serial")));
    }

    #[test]
    fn mounted_elsewhere_or_in_use_is_refused() {
        let mut d = stick("sdc", "S1", 16 * GB);
        d["partitions"][0]["mounted_at"] = json!(["/mnt/backup"]);
        assert!(one(&d).iter().any(|x| x.contains("in use at /mnt/backup")));
        let mut d = stick("sdc", "S1", 16 * GB);
        d["partitions"][0]["holders"] = json!(["dm-0"]);
        assert!(one(&d).iter().any(|x| x.contains("encryption, LVM or RAID")));
        let mut d = stick("sdc", "S1", 16 * GB);
        d["partitions"][0]["swap"] = json!(true);
        assert!(!one(&d).is_empty());
    }

    #[test]
    fn two_disks_with_one_serial_are_both_refused() {
        let all = json!([stick("sdc", "SAME", 16 * GB), stick("sdd", "SAME", 16 * GB)]);
        let j = judge(&all, 8 * GB);
        assert_eq!(j[0]["offered"], false);
        assert_eq!(j[1]["offered"], false);
        assert!(choose(&all, "SAME", 16 * GB, "Cruzer Blade", 8 * GB).is_err());
    }

    #[test]
    fn choose_needs_serial_size_and_the_typed_word() {
        let all = json!([system(), stick("sdc", "4C530001", 16 * GB)]);
        assert_eq!(choose(&all, "4C530001", 16 * GB, "Cruzer Blade", 8 * GB).unwrap()["name"], "sdc");
        assert!(choose(&all, "4C530001", 16 * GB, "yes", 8 * GB).is_err());
        assert!(choose(&all, "4C530001", 16 * GB + 512, "Cruzer Blade", 8 * GB).is_err());
        assert!(choose(&all, "GONE", 16 * GB, "Cruzer Blade", 8 * GB).unwrap_err()[0].contains("not plugged in"));
        // pointing at the system disk by its serial is still refused
        assert!(choose(&all, "EI8AN0001", 256 * GB, "SK hynix SC311", 8 * GB).is_err());
        assert!(choose(&all, "", 16 * GB, "Cruzer Blade", 8 * GB).is_err());
    }

    #[test]
    fn a_swapped_stick_under_the_same_name_is_not_written() {
        // the person picked sdc; it was pulled and another stick became sdc
        let now = json!([stick("sdc", "OTHER", 16 * GB)]);
        assert!(choose(&now, "4C530001", 16 * GB, "Cruzer Blade", 8 * GB).is_err());
    }
}

#[cfg(test)]
mod recordings {
    use super::*;

    /// A real machine's disk list, recorded with `go-back sticks --facts`.
    /// Ground truth for that machine, forever (rule #5).
    #[test]
    fn the_aspire_under_fedora_offers_nothing() {
        let facts: Value = serde_json::from_str(include_str!("../tests/corpus/acer-aspire-a515-51g-fedora-2026-09-27.json")).unwrap();
        let j = judge(&facts, 8_000_000_000);
        let j = j.as_array().unwrap();
        assert_eq!(j.len(), 2);
        assert!(j.iter().all(|d| d["offered"] == false));
        let ssd = j.iter().find(|d| d["name"] == "sdb").unwrap();
        assert!(ssd["refused_because"].to_string().contains("sdb3 is /"));
        let hdd = j.iter().find(|d| d["name"] == "sda").unwrap();
        assert!(hdd["refused_because"].to_string().contains("sda1 is /home"));
    }
}
