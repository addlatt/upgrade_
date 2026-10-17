//! `outcome`: the cutover's last step, following upgrade_/linux/outcome.sh
//! 0.6.1 line for line (architecture.md stage 2, step 11; RISKS R21). From
//! the kickstart's `%post --nochroot`. First the installed system's last
//! settings (`post.rs`, the chroot %post's shell until 0.1.3: GRUB's
//! Windows entry and grub.cfg, the start target), then: the handoff folder settle-in reads (/var/lib/upgrade_ on the
//! installed system: job.json, the Wi-Fi password files, which then leave
//! the stick), settle-in and the way back installed only when they match
//! the kit's manifest, the firmware's entries (the Windows entry re-created
//! if the firmware dropped it, Linux first), the ESP against the %pre
//! snapshot, grub.cfg listing Windows, the installed system's facts, the
//! clock, and outcome.json on the stick and in the handoff folder. Nothing
//! here aborts: a check that cannot be satisfied is written down.

use crate::util::*;
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub const FOLLOWS: &str = "0.6.1";
const STICK: &str = "/run/install/repo";
const SYSROOT: &str = "/mnt/sysroot";
const REPORT: &str = "/run/install/repo/upgrade_/report";
const SNAP: &str = "/run/install/repo/upgrade_/esp-snapshot";
const LOG: &str = "/tmp/upgrade_-outcome.log";
const HANDOFF_DIR: &str = "/var/lib/upgrade_";
const MSDATA: &str = "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7";
const SI_FILES: [&str; 6] = ["settle-in", "upgrade_-settle-in.service", "settle-in-window", "upgrade_-settle-in.desktop", "upgrade_-settle-in.sh", "org.upgrade.settle-in.policy"];
const GB_FILES: [&str; 4] = ["wimlib-imagex", "cabextract", "upgrade-gate.exe", "upgrade_-go-back-to-windows.desktop"];
const UPG_MARK: &str = include_str!("upg-mark.sh");
const UPG_MARK_SERVICE: &str = include_str!("upg-mark.service");

fn sha(path: &str) -> String {
    sha256_file(Path::new(path)).unwrap_or_default()
}

fn chmod(path: &str, mode: u32) -> bool {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).is_ok()
}

fn copy_mode(src: &str, dst: &str, mode: u32) -> bool {
    std::fs::copy(src, dst).is_ok() && chmod(dst, mode)
}

fn mkdirs(paths: &[&str]) -> bool {
    paths.iter().all(|p| std::fs::create_dir_all(p).is_ok())
}

/// `si_ok <name>`: the stick's manifest names it and the bytes match.
fn si_ok(name: &str) -> bool {
    let want = read_to_string(&format!("{STICK}/SHA256SUMS")).lines().find(|l| l.ends_with(&format!(" ./upgrade_/settle-in/{name}")) || l.ends_with(&format!("*./upgrade_/settle-in/{name}"))).map(|l| l.chars().take(64).collect::<String>()).unwrap_or_default();
    !want.is_empty() && want == sha(&format!("{STICK}/upgrade_/settle-in/{name}"))
}

fn grep_num(text: &str, pat: &dyn Fn(&str) -> bool) -> String {
    text.lines().find(|l| pat(&l.to_lowercase())).and_then(|l| l.strip_prefix("Boot")).filter(|r| r.len() >= 4 && r[..4].bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_lowercase())).map(|r| r[..4].to_string()).unwrap_or_default()
}

fn win_num(text: &str) -> String {
    grep_num(text, &|l| l.contains("bootmgfw.efi"))
}

fn lin_num(text: &str) -> String {
    grep_num(text, &|l| l.contains("shimx64.efi") || l.contains("\\efi\\fedora"))
}

fn readlink_base(path: &str) -> String {
    std::fs::read_link(path).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default()
}

pub fn main(job_path: &str) -> i32 {
    let log = Log { path: LOG.into() };
    log.say(&format!("== upgrade_ outcome.sh {FOLLOWS} (upgrade-installer {}, Rust) {}", env!("CARGO_PKG_VERSION"), utc_now()));
    let job: Value = std::fs::read_to_string(job_path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let path_chosen = jq(&job, "intent.path");
    // --- the installed system's last settings, before anything below reads them
    let p = crate::post::main(SYSROOT, &path_chosen, &jq(&job, "intent.start_at"));
    log.say(&format!("== post: start target {} set={}{}", p.target, p.target_set, p.windows_entries.map(|n| format!("; Windows entry written, grub.cfg made again ({n} line(s) name Windows)")).unwrap_or_default()));
    run("mount", &["-o", "remount,rw", STICK]);
    let converted_ok = std::fs::metadata(format!("{STICK}/upgrade_/converted")).map(|m| m.len() > 0).unwrap_or(false);
    log.say(&format!("== stick markers: boot-install={} converted={}", if exists(&format!("{STICK}/upgrade_/boot-install")) { "PRESENT" } else { "gone" }, if converted_ok { "written" } else { "MISSING" }));
    let _ = std::fs::create_dir_all(REPORT);

    // --- 0. the handoff folder settle-in reads
    let handoff = format!("{SYSROOT}{HANDOFF_DIR}");
    let wifi_rel = jq(&job, "harvest.wifi.secrets_dir");
    let wifi_expected = job.pointer("/harvest/wifi/profiles").and_then(Value::as_array).map(|l| l.iter().filter(|p| p.get("secrets_file").is_some_and(|s| !s.is_null() && s.as_str() != Some(""))).count()).unwrap_or(0);
    let (mut wifi_on_stick, mut wifi_moved) = (0usize, 0usize);
    let mut wifi_err = String::new();
    let handoff_ok = std::fs::create_dir_all(&handoff).is_ok() && chmod(&handoff, 0o700) && copy_mode(job_path, &format!("{handoff}/job.json"), 0o600);
    if !wifi_rel.is_empty() && Path::new(&format!("{STICK}/upgrade_/{wifi_rel}")).is_dir() {
        let src = format!("{STICK}/upgrade_/{wifi_rel}");
        let dst = format!("{handoff}/{wifi_rel}");
        if std::fs::create_dir_all(&dst).is_ok() {
            for d in [dst.as_str(), &format!("{handoff}/artifacts"), &format!("{handoff}/artifacts/credentials")] {
                chmod(d, 0o700);
            }
        }
        for f in sorted_entries(&src).into_iter().filter(|n| n.ends_with(".xml")) {
            let fp = format!("{src}/{f}");
            if !Path::new(&fp).is_file() {
                continue;
            }
            wifi_on_stick += 1;
            let to = format!("{dst}/{f}");
            if copy_mode(&fp, &to, 0o600) && sha(&fp) == sha(&to) {
                wifi_moved += 1;
            } else {
                wifi_err.push_str(&format!(" could not copy {f};"));
            }
        }
        let _ = std::fs::remove_dir_all(&src);
    } else if wifi_expected > 0 {
        wifi_err = format!("the job names {wifi_expected} Wi-Fi password file(s) but the stick has none");
    }
    let wifi_removed = wifi_rel.is_empty() || !exists(&format!("{STICK}/upgrade_/{wifi_rel}"));
    if wifi_on_stick != wifi_expected && wifi_err.is_empty() {
        wifi_err = format!("the job names {wifi_expected} Wi-Fi password file(s), the stick had {wifi_on_stick}");
    }
    run("chroot", &[SYSROOT, "restorecon", "-R", HANDOFF_DIR]);
    // settle-in itself, only if every file matches the kit's manifest
    let si_src = format!("{STICK}/upgrade_/settle-in");
    let mut settle_in = false;
    let mut settle_in_why = String::new();
    let si_all = SI_FILES.iter().all(|f| Path::new(&format!("{si_src}/{f}")).is_file());
    if !si_all {
        settle_in_why = "settle-in is not complete on the stick".into();
    } else if !SI_FILES.iter().all(|f| si_ok(f)) {
        settle_in_why = "settle-in on the stick does not match SHA256SUMS".into();
    } else if mkdirs(&[&format!("{SYSROOT}/usr/local/libexec/upgrade_"), &format!("{SYSROOT}/etc/systemd/system/sysinit.target.wants"), &format!("{SYSROOT}/etc/xdg/autostart"), &format!("{SYSROOT}/etc/profile.d"), &format!("{SYSROOT}/usr/share/polkit-1/actions")])
        && copy_mode(&format!("{si_src}/settle-in"), &format!("{SYSROOT}/usr/local/libexec/upgrade_/settle-in"), 0o755)
        && copy_mode(&format!("{si_src}/upgrade_-settle-in.service"), &format!("{SYSROOT}/etc/systemd/system/upgrade_-settle-in.service"), 0o644)
        && {
            let link = format!("{SYSROOT}/etc/systemd/system/sysinit.target.wants/upgrade_-settle-in.service");
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink("../upgrade_-settle-in.service", &link).is_ok()
        }
        && copy_mode(&format!("{si_src}/settle-in-window"), &format!("{SYSROOT}/usr/local/libexec/upgrade_/settle-in-window"), 0o755)
        && copy_mode(&format!("{si_src}/upgrade_-settle-in.desktop"), &format!("{SYSROOT}/etc/xdg/autostart/upgrade_-settle-in.desktop"), 0o644)
        && copy_mode(&format!("{si_src}/upgrade_-settle-in.sh"), &format!("{SYSROOT}/etc/profile.d/upgrade_-settle-in.sh"), 0o644)
        && copy_mode(&format!("{si_src}/org.upgrade.settle-in.policy"), &format!("{SYSROOT}/usr/share/polkit-1/actions/org.upgrade.settle-in.policy"), 0o644)
    {
        settle_in = true;
        run("chroot", &[SYSROOT, "restorecon", "-R", "/usr/local/libexec/upgrade_", "/etc/systemd/system/upgrade_-settle-in.service", "/etc/xdg/autostart/upgrade_-settle-in.desktop", "/etc/profile.d/upgrade_-settle-in.sh", "/usr/share/polkit-1/actions/org.upgrade.settle-in.policy"]);
    } else {
        settle_in_why = "settle-in could not be copied onto the installed system".into();
    }
    // "Go back to Windows" (R30, R33): separate on purpose
    let mut go_back = false;
    if settle_in {
        let gb_ok = GB_FILES.iter().all(|f| Path::new(&format!("{si_src}/{f}")).is_file() && si_ok(f));
        if gb_ok
            && std::fs::create_dir_all(format!("{SYSROOT}/usr/share/applications")).is_ok()
            && copy_mode(&format!("{si_src}/wimlib-imagex"), &format!("{SYSROOT}/usr/local/libexec/upgrade_/wimlib-imagex"), 0o755)
            && copy_mode(&format!("{si_src}/cabextract"), &format!("{SYSROOT}/usr/local/libexec/upgrade_/cabextract"), 0o755)
            && copy_mode(&format!("{si_src}/upgrade-gate.exe"), &format!("{SYSROOT}/usr/local/libexec/upgrade_/upgrade-gate.exe"), 0o644)
            && copy_mode(&format!("{si_src}/upgrade_-go-back-to-windows.desktop"), &format!("{SYSROOT}/usr/share/applications/upgrade_-go-back-to-windows.desktop"), 0o644)
        {
            go_back = true;
            run("chroot", &[SYSROOT, "restorecon", "-R", "/usr/local/libexec/upgrade_", "/usr/share/applications/upgrade_-go-back-to-windows.desktop"]);
        }
    }
    log.say(&format!("== go-back-to-windows installed={go_back}"));
    let settle_in_version = if settle_in { run(&format!("{SYSROOT}/usr/local/libexec/upgrade_/settle-in"), &["--version"]).out.split_whitespace().nth(1).unwrap_or("").to_string() } else { String::new() };
    log.say(&format!("== settle-in installed={settle_in} {} {settle_in_why}", if settle_in_version.is_empty() { String::new() } else { format!("version {settle_in_version}") }));
    log.say(&format!("== handoff folder {HANDOFF_DIR} ok={handoff_ok}; wifi expected={wifi_expected} on_stick={wifi_on_stick} moved={wifi_moved} removed_from_stick={wifi_removed} {}", if wifi_err.is_empty() { String::new() } else { format!("error: {wifi_err}") }));
    // erase-and-install (R27): the credentials directory goes now
    let erase = !jq(&job, "erase_consent.statement").is_empty();
    let mut scrubbed = false;
    if erase && Path::new(&format!("{STICK}/upgrade_/artifacts/credentials")).is_dir() {
        scrubbed = std::fs::remove_dir_all(format!("{STICK}/upgrade_/artifacts/credentials")).is_ok();
    } else if erase {
        scrubbed = true;
    }
    log.say(&format!("== erase-and-install={erase} credentials scrubbed={scrubbed}"));
    let espmnt = format!("{SYSROOT}/boot/efi");
    if !Path::new(&format!("{espmnt}/EFI")).is_dir() {
        log.say(&format!("!! {espmnt} is not the ESP"));
    }

    // --- 1. the firmware's entries
    let mut before = run("efibootmgr", &["-v"]).merged();
    let _ = std::fs::write("/tmp/upgrade_-efibootmgr-before.txt", &before);
    log.say_text(&before);
    let (mut win_present, mut win_recreated, mut linux_first) = (false, false, false);
    let mut w = win_num(&before);
    let mut l = lin_num(&before);
    if !w.is_empty() {
        win_present = true;
    } else {
        let espdev = run("findmnt", &["-no", "SOURCE", &espmnt]).out.trim().to_string();
        if !espdev.is_empty() && path_chosen == "keep-windows" {
            let disk = run("lsblk", &["-no", "PKNAME", &espdev]).out.lines().next().unwrap_or("").trim().to_string();
            let pr = run("lsblk", &["-no", "PARTN", &espdev]);
            let pnum = if pr.ok() { pr.out.trim().to_string() } else { read_to_string(&format!("/sys/class/block/{}/partition", Path::new(&espdev).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())).trim().to_string() };
            if run("efibootmgr", &["-c", "-d", &format!("/dev/{disk}"), "-p", &pnum, "-L", "Windows Boot Manager", "-l", "\\EFI\\Microsoft\\Boot\\bootmgfw.efi"]).ok() {
                win_present = true;
                win_recreated = true;
            }
            before = run("efibootmgr", &["-v"]).merged();
            let _ = std::fs::write("/tmp/upgrade_-efibootmgr-before.txt", &before);
            w = win_num(&before);
            l = lin_num(&before);
        }
    }
    if !l.is_empty() {
        let order = before.lines().find_map(|x| x.strip_prefix("BootOrder: ")).unwrap_or("").to_string();
        let first = order.split(',').next().unwrap_or("");
        if first == l {
            linux_first = true;
        } else {
            let rest: Vec<&str> = order.split(',').filter(|x| !x.is_empty() && *x != l).collect();
            let new = if rest.is_empty() { l.clone() } else { format!("{l},{}", rest.join(",")) };
            if run("efibootmgr", &["-o", &new]).ok() {
                linux_first = true;
            }
        }
    }
    let _ = std::fs::write(format!("{REPORT}/efibootmgr-after.txt"), run("efibootmgr", &["-v"]).merged());
    log.say(&format!("== windows entry present={win_present} recreated={win_recreated} linux_first={linux_first} (W={w} L={l})"));

    // --- 2. the ESP against the snapshot
    let mut bootmgfw_ok = false;
    let mut fallback = "other";
    if Path::new(&format!("{SNAP}/SHA256SUMS")).is_file() {
        let sums = read_to_string(&format!("{SNAP}/SHA256SUMS"));
        let want = sums.lines().find(|x| x.ends_with("EFI/Microsoft/Boot/bootmgfw.efi")).map(|x| x.chars().take(64).collect::<String>()).unwrap_or_default();
        let now = sha(&format!("{espmnt}/EFI/Microsoft/Boot/bootmgfw.efi"));
        if !want.is_empty() && want == now {
            bootmgfw_ok = true;
        }
        let mut fb_now = sha(&format!("{espmnt}/EFI/BOOT/BOOTX64.EFI"));
        if fb_now.is_empty() {
            fb_now = sha(&format!("{espmnt}/EFI/Boot/bootx64.efi"));
        }
        let fb_snap = sums.lines().find(|x| x.to_lowercase().ends_with("efi/boot/bootx64.efi")).map(|x| x.chars().take(64).collect::<String>()).unwrap_or_default();
        let shim = sha(&format!("{espmnt}/EFI/fedora/shimx64.efi"));
        if !fb_now.is_empty() && fb_now == fb_snap {
            fallback = "windows";
        } else if !fb_now.is_empty() && fb_now == shim {
            fallback = "shim";
        }
        let mut files = Vec::new();
        for d in ["EFI/Boot", "EFI/BOOT", "EFI/Microsoft"] {
            walk(&Path::new(&espmnt).join(d), d, &mut files);
        }
        files.sort();
        let after: String = files.iter().map(|f| format!("{}  {f}\n", sha(&format!("{espmnt}/{f}")))).collect();
        let _ = std::fs::write(format!("{REPORT}/esp-after.sha256"), after);
    }
    log.say(&format!("== bootmgfw matches snapshot={bootmgfw_ok} fallback loader={fallback}"));

    // --- 3. grub.cfg lists Windows
    let grub = read_to_string(&format!("{SYSROOT}/boot/grub2/grub.cfg"));
    let n = grub.lines().filter(|x| x.to_lowercase().contains("menuentry 'windows")).count();
    let grub_win = n > 0;
    let _ = std::fs::copy(format!("{SYSROOT}/boot/grub2/grub.cfg"), format!("{REPORT}/grub.cfg"));
    log.say(&format!("== grub.cfg windows entries: {n}"));

    // --- 4. facts for outcome.json
    let mut kernels = sorted_entries(&format!("{SYSROOT}/lib/modules"));
    kernels.sort_by(|a, b| version_cmp(a, b));
    let kernel = kernels.last().cloned().unwrap_or_default();
    let boot_target = readlink_base(&format!("{SYSROOT}/etc/systemd/system/default.target"));
    let dm = readlink_base(&format!("{SYSROOT}/etc/systemd/system/display-manager.service"));
    log.say(&format!("== installed system boots to: {}, display manager: {}", if boot_target.is_empty() { "unknown" } else { &boot_target }, if dm.is_empty() { "none" } else { &dm }));
    let rootdev = run("findmnt", &["-no", "SOURCE", SYSROOT]).out.trim().to_string();
    let release = read_to_string(&format!("{SYSROOT}/etc/os-release")).lines().find_map(|x| x.strip_prefix("VERSION_ID=")).map(|v| v.trim_matches('"').to_string()).unwrap_or_default();
    let (mut winpart, mut winguid, mut winsize, mut winnum) = (String::new(), String::new(), 0i64, 0i64);
    if path_chosen == "keep-windows" {
        let espdev = run("findmnt", &["-no", "SOURCE", &espmnt]).out.trim().to_string();
        let disk = run("lsblk", &["-no", "PKNAME", &espdev]).out.lines().next().unwrap_or("").trim().to_string();
        let mut best = 0i64;
        for p in sorted_entries(&format!("/sys/block/{disk}")).into_iter().filter(|x| x.starts_with(&disk) && Path::new(&format!("/sys/block/{disk}/{x}")).is_dir()) {
            let part = format!("/dev/{p}");
            if run("lsblk", &["-no", "PARTTYPE", &part]).out.trim().to_lowercase() != MSDATA {
                continue;
            }
            let sz: i64 = run("lsblk", &["-no", "SIZE", "-b", &part]).out.trim().parse().unwrap_or(0);
            if sz > best {
                best = sz;
                winpart = part.clone();
                winguid = run("lsblk", &["-no", "PARTUUID", &part]).out.trim().to_string();
                winsize = sz;
                winnum = read_to_string(&format!("/sys/block/{disk}/{p}/partition")).trim().parse().unwrap_or(0);
            }
        }
        let fstype = if winpart.is_empty() { String::new() } else { run("lsblk", &["-no", "FSTYPE", &winpart]).out.trim().to_string() };
        log.say(&format!("== windows partition: {} guid={winguid} size={winsize} (fstype {fstype})", if winpart.is_empty() { "none" } else { &winpart }));
    }
    for f in ["/tmp/anaconda.log", "/tmp/storage.log", "/tmp/program.log", "/tmp/packaging.log"] {
        let _ = std::fs::copy(f, format!("{REPORT}/{}", Path::new(f).file_name().unwrap().to_string_lossy()));
    }
    let _ = std::fs::copy(format!("{SYSROOT}/root/upgrade_-post.log"), format!("{REPORT}/post.log"));
    let _ = std::fs::copy(format!("{SYSROOT}/root/anaconda-ks.cfg"), format!("{REPORT}/anaconda-ks.cfg"));
    let _ = std::fs::copy(LOG, format!("{REPORT}/outcome.log"));
    let prologue_rec = if Path::new(&format!("{STICK}/upgrade_/prologue.json")).is_file() { format!("{STICK}/upgrade_/prologue.json") } else { String::new() };
    log.say(&format!("== prologue record: {}", if prologue_rec.is_empty() { "none (placeholders)" } else { &prologue_rec }));
    // the clock at the end of the install (cutover.clock): read, never set
    let clk_sys = epoch_now();
    let clk_rtc = read_to_string("/sys/class/rtc/rtc0/since_epoch").trim().to_string();
    let clk_ntp = run("timedatectl", &["show", "-p", "NTPSynchronized", "--value"]).out.trim().to_string();
    let clk_detail: String = run("chronyc", &["-n", "tracking"]).out.lines().filter(|x| x.starts_with("Reference ID") || x.starts_with("Leap status")).map(|x| format!("{x};")).collect();
    log.say(&format!("== clock at the end of the install: installer={clk_sys} rtc={} ntp_synchronized={} {clk_detail}", if clk_rtc.is_empty() { "unreadable" } else { &clk_rtc }, if clk_ntp.is_empty() { "unknown" } else { &clk_ntp }));

    // --- outcome.json, as the script's Python built it
    let v: Value = std::fs::read_to_string(format!("{REPORT}/verify.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}));
    let (hw, pl, snap) = (v.get("hardware").cloned().unwrap_or(json!({})), v.get("payload").cloned().unwrap_or(json!({})), v.get("esp_snapshot").cloned().unwrap_or(json!({})));
    let bl = job.pointer("/harvest/bitlocker/status").cloned().unwrap_or(Value::Null);
    let bl_on = bl.as_str() == Some("on");
    let path = if path_chosen.is_empty() { "keep-windows".to_string() } else { path_chosen.clone() };
    let keep = path == "keep-windows";
    let s = |x: &Value, k: &str, d: &str| x.get(k).and_then(Value::as_str).unwrap_or(d).to_string();
    let now = utc_now();
    let mut o = json!({
        "schema": "outcome/1", "job_id": job.get("job_id").cloned().unwrap_or(Value::Null),
        "created_utc": now,
        "converter_version": FOLLOWS,
        "status": "completed", "stopped_at": null, "reason": null,
        "path_taken": path,
        "commit_line": {"crossed": !keep, "crossed_utc": if keep { Value::Null } else { json!(now) }, "act": if keep { Value::Null } else { json!("wipe") }},
        "prologue": {
            "revalidated": true, "mismatches": [],
            "volume_check": {"needed": false, "ran": false, "disk_health_at_check": null, "method": "none", "scan": null, "restarts": 0, "wininit_1001": null, "found000_present": null, "dirty_after": "unknown"},
            "shrink": {"remeasured_gb": null, "remeasured_by": null, "remeasured_diskpart_gb": null, "fork_taken": path, "requested_bytes": null, "freed_bytes": 0},
            "bitlocker": {"status_before": bl, "suspended": bl_on, "reboot_count": if bl_on { json!(1) } else { Value::Null }},
            "handoff": {"armed": true, "marker": "boot-install", "entry_guid": null, "armed_utc": null, "bcd_backup": null}
        },
        "cutover": {
            "identity_verified": v.pointer("/identity/result").and_then(Value::as_str) == Some("pass"),
            "stick_verified": {"files": if s(&pl, "result", "") == "pass" { 1 } else { 0 }, "bytes": 0, "failed": if s(&pl, "result", "") == "pass" { json!([]) } else { json!([s(&pl, "image", "")]) }},
            "hardware": {"display": s(&hw, "display", "skipped"), "wifi": s(&hw, "wifi", "skipped"), "audio_firmware": s(&hw, "audio_firmware", "skipped"), "human_gate": "not-required"},
            "install": {"distro": "fedora", "release": if release.is_empty() { "unknown".to_string() } else { release }, "kernel": kernel, "root_partition": rootdev, "esp_reused": keep, "artifacts_injected": [],
                        "boot_target": if boot_target.is_empty() { "unknown".to_string() } else { boot_target }, "display_manager": if dm.is_empty() { Value::Null } else { json!(dm) },
                        "start_at": job.pointer("/intent/start_at").cloned().unwrap_or(json!("desktop"))},
            "boot_chain": {"windows_entry_present": win_present, "windows_entry_recreated": win_recreated, "linux_first_in_bootorder": linux_first,
                           "bootmgfw_matches_snapshot": bootmgfw_ok, "grub_lists_windows": grub_win, "fallback_loader": fallback}
        },
        "windows": {"kept": keep,
                    "partition": if winpart.is_empty() { Value::Null } else { json!({"number": winnum, "guid": winguid, "size_bytes": winsize}) },
                    "reachable_via": if keep { if win_present && grub_win { "both" } else if grub_win { "grub" } else if win_present { "firmware-entry" } else { "none" } } else { "none" }},
        "credentials": {"scrubbed": false, "scrub_after": if keep { "settle-in-pull" } else { "cutover" }},
        "logs": ["upgrade_/report/outcome.log", "upgrade_/report/anaconda.log", "upgrade_/report/storage.log", "upgrade_/report/post.log"]
    });
    if job.get("risk_acknowledgement").is_some_and(|r| !r.is_null()) {
        o["risk_acknowledgement"] = job["risk_acknowledgement"].clone();
    }
    if !prologue_rec.is_empty() {
        let taken = (|| -> Result<String, String> {
            let pr: Value = serde_json::from_str(&std::fs::read_to_string(&prologue_rec).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            if pr.get("schema").and_then(Value::as_str) != Some("prologue/1") {
                return Err(format!("prologue record schema {} is not prologue/1", py_repr(pr.get("schema"))));
            }
            if pr.get("job_id") != job.get("job_id") {
                return Err(format!("prologue record is for job {}, not {}", py_str(pr.get("job_id")), py_str(job.get("job_id"))));
            }
            let block = pr.get("prologue").cloned().ok_or("'prologue'")?;
            let fork = block.pointer("/shrink/fork_taken").and_then(Value::as_str).unwrap_or("").to_string();
            o["prologue"] = block;
            if fork == "keep-windows" || fork == "clean-slate" {
                o["path_taken"] = json!(fork);
            }
            o["logs"].as_array_mut().unwrap().push(json!("upgrade_/report/prologue.log"));
            Ok(py_str(pr.get("stage")))
        })();
        match taken {
            Ok(stage) => log.say(&format!("== prologue block taken from the prologue's record (stage {stage})")),
            Err(e) => log.say(&format!("!! prologue record unusable, placeholders kept: {e}")),
        }
    }
    if erase {
        let cd: Value = std::fs::read_to_string(format!("{REPORT}/countdown.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}));
        if cd.get("result").and_then(Value::as_str) == Some("elapsed") {
            o["commit_line"]["crossed_utc"] = cd["ended_utc"].clone();
            o["cutover"]["countdown"] = json!({"seconds": cd.get("seconds").and_then(Value::as_i64).unwrap_or(0), "result": "elapsed", "ended_utc": cd["ended_utc"]});
            o["cutover"]["hardware"]["human_gate"] = json!("countdown-elapsed");
        } else {
            log.say(&format!("!! erase job but no elapsed countdown record - the outcome will not validate: {}", py_dict(&cd)));
        }
        o["erase_consent"] = job["erase_consent"].clone();
        o["credentials"] = json!({"scrubbed": scrubbed, "scrub_after": "cutover"});
        o["logs"].as_array_mut().unwrap().push(json!("upgrade_/report/verify.log"));
    }
    o["credentials"]["wifi"] = json!({"expected": wifi_expected, "on_stick": wifi_on_stick, "moved": wifi_moved, "removed_from_stick": wifi_removed,
        "handoff_dir": if handoff_ok { json!(HANDOFF_DIR) } else { Value::Null }, "error": if wifi_err.trim().is_empty() { Value::Null } else { json!(wifi_err.trim()) }});
    o["cutover"]["settle_in"] = json!({"installed": settle_in, "version": if settle_in_version.is_empty() { Value::Null } else { json!(settle_in_version) }, "why_not": if settle_in_why.is_empty() { Value::Null } else { json!(settle_in_why) }});
    o["cutover"]["clock"] = json!({"installer_utc_epoch": clk_sys,
        "rtc_epoch": if !clk_rtc.is_empty() && clk_rtc.bytes().all(|b| b.is_ascii_digit()) { json!(clk_rtc.parse::<i64>().unwrap_or(0)) } else { Value::Null },
        "ntp_synchronized": if clk_ntp == "yes" { json!(true) } else if clk_ntp == "no" { json!(false) } else { Value::Null },
        "detail": if clk_detail.trim().is_empty() { Value::Null } else { json!(clk_detail.trim()) }});
    if keep && snap.get("result").and_then(Value::as_str) == Some("pass") {
        o["cutover"]["esp_snapshot"] = json!({"path": "upgrade_/esp-snapshot", "files": snap.get("files").and_then(Value::as_i64).filter(|n| *n != 0).unwrap_or(1), "boot_entries": "upgrade_/esp-snapshot/boot-entries.txt"});
    }
    let out_path = format!("{STICK}/upgrade_/outcome.json");
    let _ = std::fs::write(&out_path, serde_json::to_string_pretty(&o).unwrap_or_default());
    log.say(&format!("== outcome.json written: {out_path}"));
    // settle-in reads the record from the installed system (the stick may be gone by then)
    if handoff_ok && copy_mode(&out_path, &format!("{handoff}/outcome.json"), 0o600) {
        run("chroot", &[SYSROOT, "restorecon", &format!("{HANDOFF_DIR}/outcome.json")]);
    }

    // --- 5. bench instrumentation, only when the stick says so
    if Path::new(&format!("{STICK}/upgrade_/bench")).is_file() {
        let _ = std::fs::create_dir_all(format!("{SYSROOT}/usr/local/sbin"));
        let _ = std::fs::write(format!("{SYSROOT}/usr/local/sbin/upg-mark"), UPG_MARK);
        chmod(&format!("{SYSROOT}/usr/local/sbin/upg-mark"), 0o755);
        let _ = std::fs::write(format!("{SYSROOT}/etc/upg-mark.user"), format!("{}\n", jq(&job, "intent.account.linux_name")));
        let _ = std::fs::write(format!("{SYSROOT}/etc/systemd/system/upg-mark.service"), UPG_MARK_SERVICE);
        if !run("chroot", &[SYSROOT, "systemctl", "enable", "upg-mark.service"]).ok() {
            let _ = std::fs::create_dir_all(format!("{SYSROOT}/etc/systemd/system/multi-user.target.wants"));
            let link = format!("{SYSROOT}/etc/systemd/system/multi-user.target.wants/upg-mark.service");
            let _ = std::fs::remove_file(&link);
            let _ = std::os::unix::fs::symlink("/etc/systemd/system/upg-mark.service", &link);
        }
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(format!("{STICK}/upgrade_/boots.log")) {
            let _ = writeln!(f, "install-done,{}", utc_now());
        }
        log.say("== bench: boot marker installed");
    }
    let _ = std::fs::copy(LOG, format!("{REPORT}/outcome.log"));
    sync();
    sync();
    log.say("== done");
    0
}

fn walk(dir: &Path, rel: &str, out: &mut Vec<String>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).map(|r| r.flatten().collect()).unwrap_or_default();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let r = format!("{rel}/{name}");
        if e.path().is_dir() {
            walk(&e.path(), &r, out);
        } else {
            out.push(r);
        }
    }
}

/// `%r` of a JSON value, as Python's repr prints it in the script's messages.
fn py_repr(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "None".into(),
        Some(Value::String(s)) => format!("'{s}'"),
        Some(other) => other.to_string(),
    }
}

/// `%s` of a JSON value.
fn py_str(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "None".into(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// `print(..., cd)`: a dict as Python prints it (`{}` when empty).
fn py_dict(v: &Value) -> String {
    match v.as_object() {
        Some(o) if o.is_empty() => "{}".into(),
        Some(o) => format!("{{{}}}", o.iter().map(|(k, x)| format!("'{k}': {}", py_literal(x))).collect::<Vec<_>>().join(", ")),
        None => v.to_string(),
    }
}

fn py_literal(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::String(s) => format!("'{s}'"),
        Value::Number(n) => n.to_string(),
        Value::Array(l) => format!("[{}]", l.iter().map(py_literal).collect::<Vec<_>>().join(", ")),
        Value::Object(_) => py_dict(v),
    }
}
