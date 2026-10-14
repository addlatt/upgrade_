//! `verify`: the %pre verifier, following upgrade_/linux/verify.sh 0.6.0
//! line for line. It runs inside Anaconda's stage 2 before anything is
//! decided, and it is the cutover's refusal gate (architecture.md, stage 2
//! steps 5 and 7):
//!   1. IDENTITY: the disk the job names must be attached to THIS machine
//!      at exactly its size; a moved stick must never partition a
//!      stranger's laptop;
//!   2. HARDWARE: display, Wi-Fi, firmware artifacts (pass / fail / skipped);
//!   2b. the desktop image read back against the stick's manifest (R17);
//!   2c. the ESP snapshot the rollback restores from (R21);
//!   3. STORAGE: the kickstart %include for the path the job chose;
//!   4. COUNTDOWN (erase jobs, R27): two minutes on the console, a key
//!      cancels, zero is the commit line;
//!   5. the stick disarms itself at the commit line (R35).
//! Everything it learns goes to the stick under upgrade_/report/. With
//! upg.mode=verify on the command line it reports and reboots: no install.

use crate::tty::{is_char_device, Console};
use crate::util::*;
use serde_json::{json, Value};
use std::path::Path;

pub const FOLLOWS: &str = "0.6.0";
const STICK: &str = "/run/install/repo";
const REPORT: &str = "/run/install/repo/upgrade_/report";
const STORAGE_KS: &str = "/tmp/upgrade_-storage.ks";
const LOG: &str = "/tmp/upgrade_-verify.log";
const ESP_TYPE: &str = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b";
const CTTY: &str = "/dev/tty6";

struct Ctx {
    log: Log,
    mode: String,
    job_id: String,
}

/// A refusal the person can read: one screen for 60 s (any key: now), a
/// record, then the machine restarts unchanged. Never returns.
fn refuse(c: &Ctx, code: i32, plain: &str, detail: &str) -> ! {
    c.log.say(&format!("!! {detail}"));
    c.log.say(&format!("== REFUSED (exit {code}): {plain}"));
    run("mount", &["-o", "remount,rw", STICK]);
    let _ = std::fs::create_dir_all(REPORT);
    let rec = json!({"schema": "refusal/1", "exit_code": code, "reason": plain, "detail": detail, "created_utc": utc_now()});
    let _ = std::fs::write(format!("{REPORT}/refusal.json"), serde_json::to_string_pretty(&rec).unwrap_or_default());
    let _ = std::fs::copy(LOG, format!("{REPORT}/verify.log"));
    sync();
    sync();
    if is_char_device(CTTY) && run("chvt", &["6"]).ok() {
        match Console::open(CTTY) {
            Ok(con) => {
                let plain = plain.to_string();
                let screen = move |left: u64| {
                    let body = ["NOTHING WAS CHANGED ON THIS COMPUTER.".to_string(), String::new(), format!("The installer stopped before touching anything, because {plain}."), String::new(), format!("It restarts in {left} seconds, exactly as it was before."), "Press any key to restart now.".into(), String::new(), "The details are saved on the USB stick, in upgrade_\\report.".into()];
                    let mut lines = Vec::new();
                    for b in &body {
                        let w = wrap(b, 60);
                        if w.is_empty() {
                            lines.push(String::new());
                        } else {
                            lines.extend(w);
                        }
                    }
                    format!("\x1b[2J\x1b[H\n\n{}", lines.iter().map(|l| format!("   {l}\n")).collect::<String>())
                };
                con.wait(test_secs("UPG_TEST_REFUSAL_SECS", 60), &screen, false);
            }
            Err(e) => c.log.say(&format!("!! the refusal screen could not be opened ({e}); restarting in 60 s")),
        }
    } else {
        c.log.say("!! the refusal screen could not be shown on tty6; restarting in 60 s");
        sleep("60");
    }
    reboot();
    sleep("60");
    std::process::exit(code);
}

/// `norm`: lower case, no `eui.` or `0x`, no spaces, underscores, dots, hyphens.
fn norm(s: &str) -> String {
    let l = s.to_lowercase();
    let l = l.strip_prefix("eui.").map(str::to_string).unwrap_or(l);
    let l = l.strip_prefix("0x").map(str::to_string).unwrap_or(l);
    l.chars().filter(|c| !" _.-".contains(*c)).collect()
}

fn is_hex8(s: &str) -> bool {
    s.len() >= 8 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// `*-part[0-9]*`
fn is_part_link(name: &str) -> bool {
    name.find("-part").is_some_and(|i| name[i + 5..].starts_with(|c: char| c.is_ascii_digit()))
}

/// `*[0-9]p[0-9]*` or `/dev/sd[a-z]*[0-9]`
fn is_partition_node(tgt: &str) -> bool {
    let b = tgt.as_bytes();
    let np = b.windows(3).any(|w| w[0].is_ascii_digit() && w[1] == b'p' && w[2].is_ascii_digit());
    let sd = tgt.strip_prefix("/dev/sd").is_some_and(|r| r.as_bytes().first().is_some_and(u8::is_ascii_lowercase) && r.as_bytes().last().is_some_and(u8::is_ascii_digit));
    np || sd
}

fn readlink_f(path: &str) -> String {
    std::fs::canonicalize(path).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
}

fn is_block(path: &str) -> bool {
    use std::os::unix::fs::FileTypeExt;
    std::fs::metadata(path).map(|m| m.file_type().is_block_device()).unwrap_or(false)
}

/// `resolve_disk <unique_id> <serial>`: the whole-disk device carrying
/// that id, and how it was found.
fn resolve_disk(uid: &str, serial: &str) -> (String, String) {
    let mut uid_hex = norm(uid);
    if !is_hex8(&uid_hex) {
        uid_hex = String::new();
    }
    let serial_norm: String = serial.to_lowercase().chars().filter(|c| !" _.-".contains(*c)).collect();
    let links = sorted_entries("/dev/disk/by-id");
    for link in &links {
        let full = format!("/dev/disk/by-id/{link}");
        let name = link.to_lowercase();
        if is_part_link(&name) {
            continue;
        }
        let tgt = readlink_f(&full);
        if !is_block(&tgt) {
            continue;
        }
        if is_partition_node(&tgt) {
            continue;
        }
        if !uid_hex.is_empty() && uid_hex.len() >= 8 {
            let bare: String = name.chars().filter(|c| !"_.-".contains(*c)).collect();
            if bare.contains(&uid_hex) {
                return (tgt, format!("unique_id via {link}"));
            }
        }
    }
    if !serial_norm.is_empty() && serial_norm.len() >= 6 {
        for link in &links {
            let name: String = link.to_lowercase().chars().filter(|c| !"_.-".contains(*c)).collect();
            if name.find("part").is_some_and(|i| name[i + 4..].starts_with(|c: char| c.is_ascii_digit())) {
                continue;
            }
            if name.contains(&serial_norm) {
                return (readlink_f(&format!("/dev/disk/by-id/{link}")), format!("serial via {link}"));
            }
        }
    }
    (String::new(), String::new())
}

fn blockdev_size(dev: &str) -> String {
    let r = run("blockdev", &["--getsize64", dev]);
    if r.ok() { r.out.trim().to_string() } else { String::new() }
}

/// The ESP-type partitions of a disk, by sysfs: `/dev/<name>` each, in order.
fn partitions(disk: &str) -> Vec<String> {
    let dev = Path::new(disk).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    sorted_entries(&format!("/sys/block/{dev}")).into_iter().filter(|n| n.starts_with(&dev) && Path::new(&format!("/sys/block/{dev}/{n}")).is_dir()).map(|n| format!("/dev/{n}")).collect()
}

fn parttype(part: &str) -> String {
    run("lsblk", &["-no", "PARTTYPE", part]).out.trim().to_lowercase()
}

/// `mount -o ro <part> /tmp/upg-esp`, then `f`, then umount.
fn with_esp<T>(part: &str, f: impl FnOnce() -> T) -> Option<T> {
    let _ = std::fs::create_dir_all("/tmp/upg-esp");
    if !run("mount", &["-o", "ro", part, "/tmp/upg-esp"]).ok() {
        return None;
    }
    let r = f();
    run("umount", &["/tmp/upg-esp"]);
    Some(r)
}

fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    let mut entries: Vec<_> = std::fs::read_dir(src)?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        let d = dst.join(e.file_name());
        if p.is_dir() {
            copy_tree(&p, &d)?;
        } else {
            std::fs::copy(&p, &d)?;
        }
    }
    Ok(())
}

fn walk_files(dir: &Path, rel: &str, out: &mut Vec<String>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).map(|r| r.flatten().collect()).unwrap_or_default();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let r = format!("{rel}/{name}");
        if e.path().is_dir() {
            walk_files(&e.path(), &r, out);
        } else {
            out.push(r);
        }
    }
}

fn exit_code_main(label: &str, job_path: &str) -> i32 {
    let mode = if read_to_string("/proc/cmdline").split_whitespace().any(|w| w == "upg.mode=verify") { "verify" } else { "install" };
    let log = Log { path: LOG.into() };
    log.say(&format!("== upgrade_ verify.sh {FOLLOWS} (upgrade-installer {}, Rust)  mode={mode}  {}", env!("CARGO_PKG_VERSION"), utc_now()));
    log.say(&format!("== cmdline: {}", read_to_string("/proc/cmdline").trim_end()));
    let job: Value = match std::fs::read_to_string(job_path).ok().and_then(|t| serde_json::from_str(&t).ok()) {
        Some(j) => j,
        None => Value::Null,
    };
    let c = Ctx { log, mode: mode.into(), job_id: jq(&job, "job_id") };
    let _ = label;

    // --- 0. the job
    let schema = jq(&job, "schema");
    if schema != "job/1" {
        refuse(&c, 10, "this USB stick was prepared by a different version of this tool", &format!("job schema '{schema}' is not job/1"));
    }
    let path_chosen = jq(&job, "intent.path");
    let j_serial = jq(&job, "identity.system_disk.serial_number");
    let j_uid = jq(&job, "identity.system_disk.unique_id");
    let j_size = jq(&job, "identity.system_disk.size_bytes");
    c.log.say(&format!("== job {} path={path_chosen} disk serial='{j_serial}' unique_id='{j_uid}' size={j_size}", c.job_id));

    // --- 1. identity
    let (disk, matched_by) = resolve_disk(&j_uid, &j_serial);
    let mut identity = "fail";
    let mut disk_size = String::new();
    if !disk.is_empty() {
        disk_size = blockdev_size(&disk);
        c.log.say(&format!("== candidate {disk} ({matched_by}) size={disk_size}"));
        if disk_size == j_size {
            identity = "pass";
        } else {
            c.log.say(&format!("!! size mismatch: job says {j_size}, disk is {disk_size}"));
        }
    } else {
        c.log.say("!! no attached disk carries the job's unique id or serial");
    }
    let erase = !jq(&job, "erase_consent.statement").is_empty();
    let (mut home_disk, mut home_by, mut home_size) = (String::new(), String::new(), String::new());
    let ndisks = if erase { jq_len(&job, "erase_consent.disks") } else { 0 };
    if erase {
        if jq(&job, "erase_consent.disks.0.role") != "system" || jq(&job, "erase_consent.disks.0.unique_id") != j_uid || jq(&job, "erase_consent.disks.0.size_bytes") != j_size {
            c.log.say("!! erase_consent.disks[0] is not the system disk the job identifies");
            identity = "fail";
        }
        if ndisks >= 2 {
            let (h_uid, h_serial, h_size_j) = (jq(&job, "erase_consent.disks.1.unique_id"), jq(&job, "erase_consent.disks.1.serial_number"), jq(&job, "erase_consent.disks.1.size_bytes"));
            let (hd, hb) = resolve_disk(&h_uid, &h_serial);
            home_disk = hd;
            home_by = hb;
            if home_disk.is_empty() {
                c.log.say(&format!("!! the home drive the job names (serial '{h_serial}') is not attached"));
                identity = "fail";
            } else {
                home_size = blockdev_size(&home_disk);
                c.log.say(&format!("== home candidate {home_disk} ({home_by}) size={home_size}"));
                if home_size != h_size_j {
                    c.log.say(&format!("!! home size mismatch: job says {h_size_j}, disk is {home_size}"));
                    identity = "fail";
                }
                if home_disk == disk {
                    c.log.say("!! the home drive resolved to the system drive");
                    identity = "fail";
                }
            }
        }
        c.log.say(&format!("== erase-and-install: {ndisks} drive(s); identity={identity}"));
    }
    let lsblk = run("lsblk", &["-b", "-o", "NAME,SIZE,TYPE,FSTYPE,LABEL,PARTTYPENAME,SERIAL,WWN"]);
    c.log.say_text(&lsblk.out);
    let _ = std::fs::write("/tmp/upgrade_-lsblk.txt", &lsblk.out);
    let _ = std::fs::write("/tmp/upgrade_-by-id.txt", run("ls", &["-l", "/dev/disk/by-id/"]).merged());

    // --- 2. hardware
    let mut display_result = "skipped";
    let mut display_detail = String::new();
    let connectors: Vec<String> = sorted_entries("/sys/class/drm").into_iter().filter(|n| n.starts_with("card") && n.contains('-')).collect();
    for con in &connectors {
        let st = format!("/sys/class/drm/{con}/status");
        if !exists(&st) {
            continue;
        }
        if read_to_string(&st).trim_end_matches('\n') == "connected" {
            let mode = read_to_string(&format!("/sys/class/drm/{con}/modes")).lines().next().unwrap_or("").to_string();
            display_detail.push_str(&format!(" {con}:{}", if mode.is_empty() { "nomode" } else { &mode }));
            if !mode.is_empty() {
                display_result = "pass";
            }
        }
    }
    if display_result == "skipped" && !connectors.is_empty() {
        display_result = "fail";
        display_detail = "connectors present, none connected with a mode".into();
    }
    if display_detail.is_empty() {
        display_detail = sorted_entries("/sys/class/graphics").iter().map(|g| format!("{g} ")).collect();
    }
    let _ = std::fs::write("/tmp/upgrade_-fb0.txt", std::fs::read("/sys/class/graphics/fb0/virtual_size").unwrap_or_default());

    let mut wifi_result = "skipped";
    let mut wifi_detail = "no wifi device".to_string();
    if have("nmcli") {
        let devs = run("nmcli", &["-t", "-f", "DEVICE,TYPE", "device"]);
        let wdev = devs.out.lines().filter_map(|l| l.split_once(':')).find(|(_, t)| *t == "wifi").map(|(d, _)| d.to_string());
        if let Some(wdev) = wdev {
            run("nmcli", &["device", "wifi", "rescan", "ifname", &wdev]);
            sleep("4");
            let list = run("nmcli", &["-t", "-f", "SSID", "device", "wifi", "list", "ifname", &wdev]);
            let n = list.out.lines().filter(|l| !l.is_empty()).count();
            wifi_detail = format!("{wdev} sees {n} networks");
            wifi_result = if n > 0 { "pass" } else { "fail" };
        }
    }
    let _ = std::fs::write("/tmp/upgrade_-nmcli.txt", run("nmcli", &["device"]).out);

    let mut audio_result = "skipped";
    let mut audio_detail = "job lists no firmware artifacts".to_string();
    let nart = jq_len(&job, "harvest.firmware_artifacts");
    if nart > 0 {
        audio_result = "pass";
        audio_detail = String::new();
        for i in 0..nart {
            let p = jq(&job, &format!("harvest.firmware_artifacts.{i}.path"));
            if !Path::new(&format!("{STICK}/upgrade_/{p}")).is_file() {
                audio_result = "fail";
                audio_detail.push_str(&format!(" missing:{p}"));
            }
        }
    }
    let _ = std::fs::write("/tmp/upgrade_-lspci.txt", run("lspci", &["-nn"]).merged());
    let _ = std::fs::write("/tmp/upgrade_-dmesg.txt", run("dmesg", &[]).merged());

    // --- 2b. the desktop image, read back against the stick's manifest (R17)
    let desktop = jq(&job, "intent.desktop");
    let img_rel = format!("upgrade_/LiveOS/{desktop}.squashfs");
    let mut image_result = "fail";
    let image_detail;
    let mut image_mbps = String::new();
    let img_path = format!("{STICK}/{img_rel}");
    if !Path::new(&img_path).is_file() {
        image_detail = format!("missing: {img_rel}");
    } else if !Path::new(&format!("{STICK}/SHA256SUMS")).is_file() {
        image_detail = "no SHA256SUMS on the stick".into();
    } else {
        let want = read_to_string(&format!("{STICK}/SHA256SUMS")).lines().find(|l| l.ends_with(&format!(" ./{img_rel}")) || l.ends_with(&format!("\t./{img_rel}")) || l.ends_with(&format!(" {img_rel}")) || l.ends_with(&format!("\t{img_rel}"))).map(|l| l.chars().take(64).collect::<String>()).unwrap_or_default();
        if want.is_empty() {
            image_detail = format!("{img_rel} not in SHA256SUMS");
        } else {
            let bytes = std::fs::metadata(&img_path).map(|m| m.len()).unwrap_or(0);
            let t0 = std::time::Instant::now();
            let got = sha256_file(Path::new(&img_path)).unwrap_or_default();
            let secs = t0.elapsed().as_secs_f64().max(0.001);
            let mbps = (bytes as f64 / 1e6 / secs * 10.0).round() / 10.0;
            image_mbps = format!("{mbps:.1}");
            if got == want {
                image_result = "pass";
                image_detail = format!("{img_rel} {bytes} bytes sha256 ok, read at {image_mbps} MB/s");
            } else {
                image_detail = format!("{img_rel} sha256 MISMATCH (want {want} got {got})");
            }
        }
    }
    c.log.say(&format!("== desktop image: {image_result} - {image_detail}"));

    // --- 2c. the ESP snapshot (R21)
    let mut snap_result = "skipped";
    let mut snap_files = 0usize;
    let snap_dir = format!("{REPORT}/../esp-snapshot");
    if identity == "pass" && path_chosen == "keep-windows" {
        run("mount", &["-o", "remount,rw", STICK]);
        let _ = std::fs::remove_dir_all(&snap_dir);
        let _ = std::fs::create_dir_all(&snap_dir);
        snap_result = "fail";
        for part in partitions(&disk) {
            if parttype(&part) != ESP_TYPE {
                continue;
            }
            let taken = with_esp(&part, || {
                if !Path::new("/tmp/upg-esp/EFI/Microsoft/Boot/bootmgfw.efi").is_file() {
                    return false;
                }
                for d in ["EFI/Boot", "EFI/BOOT", "EFI/Microsoft"] {
                    if Path::new(&format!("/tmp/upg-esp/{d}")).is_dir() {
                        let _ = std::fs::create_dir_all(Path::new(&snap_dir).join(d).parent().unwrap());
                        let _ = copy_tree(Path::new(&format!("/tmp/upg-esp/{d}")), &Path::new(&snap_dir).join(d));
                    }
                }
                let mut files = Vec::new();
                walk_files(Path::new(&snap_dir), ".", &mut files);
                files.retain(|f| !f.ends_with("/SHA256SUMS") && !f.ends_with("/boot-entries.txt"));
                files.sort();
                let sums: String = files.iter().map(|f| format!("{}  {f}\n", sha256_file(&Path::new(&snap_dir).join(f)).unwrap_or_default())).collect();
                let _ = std::fs::write(format!("{snap_dir}/SHA256SUMS"), &sums);
                snap_files = sums.lines().filter(|l| !l.is_empty()).count();
                let _ = std::fs::write(format!("{snap_dir}/boot-entries.txt"), run("efibootmgr", &["-v"]).merged());
                let _ = std::fs::write(format!("{snap_dir}/SOURCE"), format!("disk={disk} esp={part} taken_utc={}\n", utc_now()));
                true
            });
            if taken == Some(true) {
                snap_result = "pass";
                break;
            }
        }
        sync();
    }
    c.log.say(&format!("== esp snapshot: {snap_result} ({snap_files} files) -> {snap_dir}"));

    // --- 3. the storage %include
    let mut esp = String::new();
    let mut esp_result = "skipped";
    if identity == "pass" {
        let dev = Path::new(&disk).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        for part in partitions(&disk) {
            if parttype(&part) == ESP_TYPE {
                let has = with_esp(&part, || Path::new("/tmp/upg-esp/EFI/Microsoft/Boot/bootmgfw.efi").is_file());
                if has == Some(true) {
                    esp = part;
                }
                if !esp.is_empty() {
                    break;
                }
            }
        }
        if path_chosen == "keep-windows" {
            if !esp.is_empty() {
                esp_result = "pass";
                let bl_extra = if exists(&format!("{STICK}/upgrade_/bench")) { " --timeout=20" } else { "" };
                let esp_name = Path::new(&esp).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let _ = std::fs::write(STORAGE_KS, format!("# written by verify.sh: keep-windows on {disk} ({matched_by}), ESP {esp} reused unformatted\nignoredisk --only-use={dev}\nbootloader --location=mbr --boot-drive={dev}{bl_extra}\npart /boot/efi --onpart={esp_name} --noformat\npart /boot --fstype=ext4 --size=1024 --ondisk={dev}\npart /     --fstype=ext4 --size=4096 --grow --ondisk={dev}\n"));
            } else {
                esp_result = "fail";
                c.log.say(&format!("!! keep-windows: no EFI partition holding bootmgfw.efi on {disk}"));
            }
        } else if erase {
            esp_result = "skipped";
            let hdev = if home_disk.is_empty() { String::new() } else { Path::new(&home_disk).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() };
            let drives = if hdev.is_empty() { dev.clone() } else { format!("{dev},{hdev}") };
            let mut t = format!("# written by verify.sh: erase-and-install on {drives} - the countdown before this is the commit line (R27)\nzerombr\nignoredisk --only-use={drives}\nclearpart --all --initlabel --drives={drives}\nbootloader --location=mbr --boot-drive={dev}\npart /boot/efi --fstype=efi --size=600 --ondisk={dev}\npart /boot     --fstype=ext4 --size=1024 --ondisk={dev}\npart /         --fstype=ext4 --size=4096 --grow --ondisk={dev}\n");
            if !hdev.is_empty() {
                t.push_str(&format!("part /home     --fstype=ext4 --size=1024 --grow --ondisk={hdev}\n"));
            }
            let _ = std::fs::write(STORAGE_KS, t);
        } else {
            esp_result = if esp.is_empty() { "skipped" } else { "pass" };
            let _ = std::fs::write(STORAGE_KS, format!("# written by verify.sh: clean-slate on {disk} ({matched_by}) - the wipe is the commit line\nignoredisk --only-use={dev}\nclearpart --all --initlabel --drives={dev}\nbootloader --location=mbr --boot-drive={dev}\npart /boot/efi --fstype=efi --size=600 --ondisk={dev}\npart /boot     --fstype=ext4 --size=1024 --ondisk={dev}\npart /         --fstype=ext4 --grow --ondisk={dev}\n"));
        }
        c.log.say("== storage include:");
        if exists(STORAGE_KS) {
            c.log.say_text(&read_to_string(STORAGE_KS));
        } else {
            // the script's `cat` said so
            c.log.say(&format!("cat: {STORAGE_KS}: No such file or directory"));
        }
    }

    // --- the report, on the stick
    if !run("mount", &["-o", "remount,rw", STICK]).ok() {
        c.log.say("!! could not remount the stick read-write");
    }
    let _ = std::fs::create_dir_all(REPORT);
    let secure_boot = std::fs::read("/sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c").ok().and_then(|b| b.last().map(|x| x.to_string())).unwrap_or_default();
    let r = json!({
        "schema": "verify/1", "verify_version": FOLLOWS, "verify_program": format!("upgrade-installer {} (Rust; follows verify.sh {FOLLOWS})", env!("CARGO_PKG_VERSION")), "job_id": c.job_id, "mode": mode,
        "created_utc": utc_now(),
        "kernel": run("uname", &["-r"]).out.trim_end(), "cmdline": read_to_string("/proc/cmdline").trim_end(),
        "identity": {"result": identity, "disk": disk, "matched_by": matched_by, "disk_size_bytes": disk_size, "job_size_bytes": j_size},
        "hardware": {"display": display_result, "display_detail": display_detail.trim(), "wifi": wifi_result, "wifi_detail": wifi_detail, "audio_firmware": audio_result, "audio_detail": audio_detail.trim()},
        "storage": {"path": path_chosen, "esp": esp, "esp_result": esp_result, "include_written": exists(STORAGE_KS), "erase": erase, "home_disk": home_disk, "home_matched_by": home_by, "home_size_bytes": home_size},
        "esp_snapshot": {"result": snap_result, "files": snap_files, "path": "upgrade_/esp-snapshot"},
        "payload": {"desktop": desktop, "image": img_rel, "result": image_result, "detail": image_detail, "read_mbps": image_mbps},
        "secure_boot": secure_boot,
    });
    let _ = std::fs::write(format!("{REPORT}/verify.json"), serde_json::to_string_pretty(&r).unwrap_or_default());
    let _ = std::fs::copy(STORAGE_KS, format!("{REPORT}/storage.ks"));
    for f in ["verify", "lsblk", "by-id", "fb0", "nmcli", "lspci", "dmesg"] {
        let _ = std::fs::copy(format!("/tmp/upgrade_-{f}.txt"), format!("{REPORT}/{f}.txt"));
    }
    let _ = std::fs::copy(LOG, format!("{REPORT}/verify.log"));
    sync();
    sync();
    c.log.say(&format!("== report written to {REPORT}; identity={identity} display={display_result} wifi={wifi_result} audio={audio_result} esp={esp_result} image={image_result}"));

    if c.mode == "verify" {
        c.log.say("== verify mode: rebooting to Windows (nothing installed, nothing changed on the internal disk)");
        sleep("2");
        sync();
        reboot();
        sleep("60");
        return 0;
    }
    // install mode: the refusals, in the script's order
    let converted = format!("{STICK}/upgrade_/converted");
    if exists(&converted) {
        let head: String = std::fs::read(&converted).unwrap_or_default().into_iter().take(200).map(|b| b as char).collect::<String>().replace('\n', " ");
        refuse(&c, 28, "this USB stick has already converted this computer, and it never installs twice. Take the stick out and restart", &format!("the stick carries upgrade_/converted ({head}) - refusing a second install (R35)"));
    }
    if identity != "pass" {
        refuse(&c, 20, "this USB stick was prepared for a different computer, or one of its drives has changed since", "IDENTITY MISMATCH - refusing to install on this machine");
    }
    if !exists(STORAGE_KS) {
        refuse(&c, 21, "the installer could not work out where to put Linux", "no storage include written");
    }
    if esp_result == "fail" {
        refuse(&c, 22, "Windows' startup files were not where the preparation found them", "keep-windows needs the Windows ESP");
    }
    if image_result != "pass" {
        refuse(&c, 23, "the copy of Linux on this USB stick is damaged", "the desktop image on the stick did not verify (RISKS R17)");
    }
    if path_chosen == "keep-windows" && snap_result != "pass" {
        refuse(&c, 24, "a safety copy of Windows' startup files could not be made", "the ESP snapshot failed - the ESP is not touched without it (RISKS R21)");
    }
    if path_chosen != "keep-windows" && !erase {
        refuse(&c, 25, "this version cannot yet put your files back on an erased computer", "a clean slate with staged files needs the restore, which this version does not have");
    }

    // --- 4. the countdown: the last exit before the erase (R27, rule #3)
    if erase {
        let count_secs = test_secs("UPG_TEST_COUNTDOWN", 120);
        let what = if home_disk.is_empty() { "THIS COMPUTER'S DRIVE" } else { "BOTH DRIVES" };
        let model = |d: &str| run("lsblk", &["-dno", "MODEL", d]).out.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut names = model(&disk);
        if !home_disk.is_empty() {
            names = format!("{names} and {}", model(&home_disk));
        }
        let record = |result: &str| {
            let rec = json!({"schema": "countdown/1", "result": result, "seconds": count_secs as i64, "ended_utc": utc_now()});
            let _ = std::fs::write(format!("{REPORT}/countdown.json"), serde_json::to_string_pretty(&rec).unwrap_or_default());
            let _ = std::fs::copy(LOG, format!("{REPORT}/verify.log"));
            sync();
            sync();
        };
        if !is_char_device(CTTY) || !run("chvt", &["6"]).ok() {
            let chvt = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("chvt")).find(|p| p.is_file())).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "missing".into());
            refuse(&c, 26, "the last-chance countdown could not be shown, and nothing is erased without it", &format!("the countdown could not be shown on {CTTY} (chvt: {chvt}) - no erase without a visible last exit"));
        }
        c.log.say(&format!("== countdown: {} s on {CTTY}, erasing {what} ({names})", count_secs as i64));
        let mut con = match Console::open(CTTY) {
            Ok(k) => k,
            Err(e) => {
                c.log.say(&format!("== countdown could not open the console: {e}"));
                refuse(&c, 26, "the last-chance countdown could not be shown, and nothing is erased without it", "the countdown could not run (python exit 1) - no erase without a visible last exit");
            }
        };
        c.log.say(&format!("== countdown console {}x{}", con.cols, con.rows));
        if con.cols < 40 {
            match con.resize(25, 80) {
                Ok(()) => c.log.say(&format!("== countdown console set to {}x{}", con.cols, con.rows)),
                Err(e) => c.log.say(&format!("== countdown console could not be resized: {e}")),
            }
        }
        let width = (con.cols as usize).min(100).saturating_sub(6).max(20);
        let names_c = names.clone();
        let screen = move |left: u64| {
            let mut body = vec![format!("ERASING {what} IN {}:{:02}", left / 60, left % 60), String::new(), "Windows and everything on this computer will be deleted.".to_string()];
            if !names_c.is_empty() {
                body.push(format!("({names_c})"));
            }
            body.push(String::new());
            body.push("Press any key to CANCEL and restart into Windows.".into());
            let mut lines = Vec::new();
            for b in &body {
                let w = wrap(b, width);
                if w.is_empty() {
                    lines.push(String::new());
                } else {
                    lines.extend(w);
                }
            }
            format!("\x1b[2J\x1b[H\n\n{}", lines.iter().map(|l| format!("   {l}\n")).collect::<String>())
        };
        let (keyed, elapsed) = con.wait(count_secs, &screen, true);
        let result = if keyed { "cancelled" } else { "elapsed" };
        c.log.say(&format!("== countdown {result} after {elapsed:.1} s"));
        if keyed {
            con.put("\x1b[2J\x1b[H\n\n   CANCELLED. Nothing was erased. Restarting into Windows...\n");
            drop(con);
            c.log.say("== countdown CANCELLED by a key press - nothing erased; restarting into Windows");
            record("cancelled");
            run("mount", &["-o", "remount,rw", STICK]);
            let _ = std::fs::remove_file(format!("{STICK}/upgrade_/boot-install"));
            sync();
            if exists(&format!("{STICK}/upgrade_/boot-install")) {
                c.log.say("!! boot-install could not be removed after the cancel");
            } else {
                c.log.say("== boot-install removed from the stick");
            }
            let _ = std::fs::copy(LOG, format!("{REPORT}/verify.log"));
            sync();
            sleep("3");
            reboot();
            sleep("60");
            return 27;
        }
        con.put("\x1b[2J\x1b[H\n\n   Erasing and installing Fedora. This takes a while; you can walk away.\n");
        drop(con);
        c.log.say(&format!("== countdown ELAPSED - the commit line: the installer now erases {what}"));
        record("elapsed");
        run("chvt", &["1"]);
    }

    // --- 5. the stick disarms itself at the commit line (R35)
    run("mount", &["-o", "remount,rw", STICK]);
    let _ = std::fs::write(&converted, format!("verify {FOLLOWS} job {} committed {}\n", c.job_id, utc_now()));
    let _ = std::fs::remove_file(format!("{STICK}/upgrade_/boot-install"));
    sync();
    sync();
    let written = std::fs::metadata(&converted).map(|m| m.len() > 0).unwrap_or(false);
    if !written || exists(&format!("{STICK}/upgrade_/boot-install")) {
        let _ = std::fs::remove_file(&converted);
        refuse(&c, 29, "this USB stick could not be switched off as an installer, and without that it could install again by itself", "could not write upgrade_/converted or remove upgrade_/boot-install on the stick - no install without a disarmed stick (R35)");
    }
    c.log.say("== the stick is disarmed: upgrade_/converted written, upgrade_/boot-install removed");
    let _ = std::fs::copy(LOG, format!("{REPORT}/verify.log"));
    sync();
    0
}

pub fn main(job_path: &str, label: &str) -> i32 {
    exit_code_main(label, job_path)
}
