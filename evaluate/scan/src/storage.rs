//! The storage checks: the controller's mode, room to keep Windows, the
//! drive's health, the volume's health, Fast Startup, BitLocker and the
//! boot partition. These carry the R18 and R21 guardrails, so the wording
//! and the order of the rules follow the PowerShell line by line.

use crate::check::{Scan, Status};
use crate::data::tables;
use crate::facts::{BitLockerState, DiskFacts, EspFacts, PhysicalDiskFacts, Pnp, VolumeHealth};
use crate::parse::{ntfs98_fresh, pci_id, shrink_mitigable};
use crate::ps::{eq_ci, matches, n, num, round1, s, starts_with_ci, truthy};

const GB: f64 = 1073741824.0;
const MB: f64 = 1048576.0;

const AHCI_REMEDY: &str = "Switch the BIOS from RST/VMD to AHCI. Windows will not boot afterwards unless
you prepare it first, so do this only when you are committed:

  1. Open an Administrator Command Prompt in Windows and run:  bcdedit /set safeboot minimal
  2. Reboot into BIOS. Find SATA/NVMe/VMD mode and set it to AHCI.
  3. Let Windows boot into Safe Mode once - it installs the AHCI driver.
  4. Administrator Command Prompt again:  bcdedit /deletevalue safeboot
  5. Reboot. Windows now runs in AHCI mode and Linux installers can see the disk.

If you are wiping Windows entirely you can skip steps 1-4 and simply switch to
AHCI - but then Windows will not boot, so there is no going back.";

/// Three signals, strongest first (reconciled 2026-08-22, R1/V5): a VMD
/// device ID from the kernel's own table; the VMD driver service (iaStorVD)
/// on any device; an Intel controller that reports RAID class 0104. An
/// iaStor* driver on a controller that is not in RAID mode only warns.
pub fn storage_mode(scan: &mut Scan, pnp: &[Pnp]) {
    const T: &str = "Storage controller mode";
    let vmd = &tables().vmd_device_ids;
    let (mut hit, mut raid_hit, mut rst_hit): (Option<&Pnp>, Option<&Pnp>, Option<&Pnp>) = (None, None, None);
    for d in pnp {
        if !truthy(&d.device_id) {
            continue;
        }
        let id = pci_id(s(&d.device_id));
        if id.as_ref().is_some_and(|i| vmd.iter().any(|v| eq_ci(v, i))) {
            hit = Some(d);
            break;
        }
        if starts_with_ci(s(&d.service), "iaStorVD") {
            hit = Some(d);
            break;
        }
        let intel = id.as_ref().is_some_and(|i| i.starts_with("8086:"));
        if intel && raid_hit.is_none() && d.compatible_id.iter().any(|c| matches("CC_0104", c)) {
            raid_hit = Some(d);
        }
        if rst_hit.is_none() && starts_with_ci(s(&d.service), "iaStor") {
            rst_hit = Some(d);
        }
    }
    if let Some(h) = hit.or(raid_hit) {
        scan.add("Storage", T, Status::Fail, format!("Intel RST / VMD active ({})", s(&h.name)))
            .note("Your SSD is behind Intel RST/VMD. Linux installers will show no disks at all - the drive is simply invisible. This is the single most common reason a Linux install appears to fail on modern laptops, and it looks like a broken installer rather than a setting.")
            .remedy(AHCI_REMEDY);
        return;
    }
    if let Some(h) = rst_hit {
        scan.add("Storage", T, Status::Warn, format!("Intel RST driver present, controller not in RAID mode ({})", s(&h.service)))
            .note("Windows is using an Intel RST storage driver, but the disk controller reports plain AHCI/NVMe, so Linux installers should see the disk normally. This combination has not been confirmed on real hardware yet - please report what you find.")
            .remedy("Before wiping anything, boot the Linux installer USB and confirm it lists your internal disk. If it shows no disks, look in the BIOS for a setting named VMD, RST, Optane or \"RAID mode\" and set it to AHCI.");
        return;
    }
    scan.add("Storage", T, Status::Ok, "standard AHCI / NVMe - visible to Linux installers");
}

/// Test-UpgDisk. `repair_queued` comes from `parse::repair_queued`.
pub fn disk(scan: &mut Scan, facts: &DiskFacts, is_admin: bool, repair_queued: bool) {
    const RK: &str = "Room to keep Windows";
    for d in &facts.disks {
        let size = round1(d.size.unwrap_or(0.0) / GB);
        scan.add("Storage", &format!("Disk {}", n(d.number)), Status::Info, format!("{} - {} GB, {}, {}", s(&d.friendly_name), num(size), s(&d.partition_style), s(&d.bus_type)));
    }
    let Some(sys) = &facts.sys_volume else {
        scan.add("Storage", "Disk space", Status::Unknown, "could not read C:");
        return;
    };
    let free = round1(sys.size_remaining / GB);
    let used = round1((sys.size - sys.size_remaining) / GB);
    // No external drive anywhere in this design, so the scanner reports what
    // is used and how far the disk can shrink, never "buy a backup drive".
    scan.add("Storage", "Disk in use", Status::Info, format!("{} GB used, {} GB free", num(used), num(free)))
        .note("Your personal files are part of that used space; the rest is Windows and installed programs. The converter puts your files on a USB stick, or leaves them in place while it fits Linux alongside. It never needs an external hard drive.");

    let last = s(&facts.last_unmovable);
    if let Some(shrink) = facts.shrink_gb {
        let gbs = num(shrink);
        // when the number came from diskpart, say so, and carry the reason
        // the first path refused
        let mut via = String::new();
        if eq_ci(s(&facts.shrink_source), "diskpart") {
            via.push_str(" (measured via diskpart");
            if truthy(&facts.shrink_error) {
                via.push_str(&format!("; the Storage API path said: {}", s(&facts.shrink_error)));
            }
            via.push(')');
        }
        if repair_queued && shrink < 25.0 {
            // not a measurement (R18, 2026-09-17): never steer toward clean slate on it
            scan.add("Storage", RK, Status::Info, format!("Windows answered {gbs} GB{via}, but a full disk check is queued - not a trustworthy number"))
                .note("Windows has an offline repair of C: queued (see 'Volume health' below). Until that check has run, its shrink answer is not a measurement: the first machine that showed this answered 0 GB with no error while 33 GB were free. The converter runs the check itself, with its own restart, then measures again - whether Windows can be kept is decided on that number, not this one.")
                .remedy("Nothing to do now. If you want the number before converting: open an Administrator prompt, run \"chkdsk C: /f\", answer Y, restart, and run this scanner again.");
        } else if shrink < 25.0 && shrink_mitigable(last) {
            // the cold floor with a file the converter turns off (R18, 2026-09-20)
            scan.add("Storage", RK, Status::Info, format!("{gbs} GB can be freed as things stand{via} - Windows names {last} as the file in the way"))
                .note(format!("Windows keeps its hibernation, page and swap files wherever they landed, and nothing behind the last of them can be released - here that file is {last}. That is ordinary and says nothing about how full the disk is. The converter turns those files off, restarts once and measures again; whether Windows can be kept is decided on that second number, not this one."))
                .remedy("Nothing to do now - the converter handles this itself.");
        } else if shrink < 25.0 {
            let pinned = if last.is_empty() { String::new() } else { format!(" Windows names the last unmovable file: {last}.") };
            scan.add("Storage", RK, Status::Warn, format!("{gbs} GB can be freed by shrinking{via}"))
                .note(format!("Too little room to install Linux while keeping Windows as a fallback. This machine can still convert - your files travel on the USB stick (the clean-slate path) - but there is no space to keep a safety copy of Windows on the internal disk.{pinned}"))
                .remedy("Emptying the Recycle Bin, clearing Downloads, and removing large unused programs raises this number. No external drive is needed either way.");
        } else {
            scan.add("Storage", RK, Status::Ok, format!("{gbs} GB can be freed by shrinking{via}"))
                .note("Enough room to install Linux while keeping Windows shrunk aside as a fallback, until you confirm everything works and reclaim the space.");
        }
    } else if !is_admin {
        scan.add("Storage", RK, Status::Info, "requires Administrator to measure")
            .note("Measuring how far the disk can shrink needs Administrator rights. Without it, we cannot yet tell you whether Windows can be kept as a fallback - the clean-slate path (files on the USB stick) still works regardless.")
            .remedy("Re-run this scanner as Administrator to get this number.");
    } else {
        // Report what Windows said. No cause is named: the obvious suspect
        // (Fast Startup) is contradicted by the rig's own evidence.
        let why = if truthy(&facts.shrink_error) { format!("Windows reported: {}", s(&facts.shrink_error)) } else { "Windows returned no value and no error.".to_string() };
        let mut at = if truthy(&facts.shrink_failed_at) { format!(" (failed at {})", s(&facts.shrink_failed_at)) } else { String::new() };
        if truthy(&facts.diskpart_error) {
            at.push_str(&format!(" diskpart's shrink querymax also gave nothing usable: {}.", s(&facts.diskpart_error)));
        }
        if matches("volume with errors|corrupt|chkdsk", &format!("{} {}", s(&facts.shrink_error), s(&facts.diskpart_error))) {
            at.push_str(" That is the volume flag - see 'Volume health' below; once the disk check has run, this number appears.");
        }
        scan.add("Storage", RK, Status::Info, "could not measure shrinkable space")
            .note(format!("{why}{at} We do not yet know why this happens on some machines, so we are not going to guess - the converter measures again before it does anything, and keeping Windows is only offered if the number is there. The clean-slate path (your files on the USB stick) does not depend on this."))
            .remedy("If you are reporting this machine to the project, include this line - the exact wording above is the useful part.");
    }

    let parts = facts.disk0_part_count;
    let mbr = facts.disks.first().is_some_and(|d| eq_ci(s(&d.partition_style), "MBR"));
    if mbr && parts.is_some_and(|p| p >= 4) {
        scan.add("Storage", "Partition table", Status::Warn, format!("MBR with {} primary partitions", n(parts)))
            .note("MBR discs allow only four primary partitions and you are at the limit. The installer will not be able to create a new one.")
            .remedy("Delete or convert a partition, or erase the disk and let the installer create a fresh GPT layout.");
    }
}

/// Test-UpgPhysicalDisk. Windows' health flag is lax (the Aspire said
/// Healthy with 725 uncorrectable reads and 261 logged bad blocks), so the
/// error log and the SMART counters decide before the flag does.
pub fn physical_disk(scan: &mut Scan, facts: Option<&PhysicalDiskFacts>) {
    const T: &str = "Disk health";
    let Some(f) = facts.filter(|f| f.found) else {
        let why = facts.filter(|f| truthy(&f.error)).map(|f| format!(" ({})", s(&f.error))).unwrap_or_default();
        scan.add("Storage", T, Status::Unknown, "could not read the drive health")
            .note(format!("Windows did not report a health status for the drive that holds C:{why}. The converter reads this again before it does anything and refuses the disk-check step without it."));
        return;
    };
    let mut counters = String::new();
    if let Some(c) = &f.counters {
        let mut bits = Vec::new();
        if let Some(x) = c.read_errors_uncorrected {
            bits.push(format!("uncorrected read errors {x}"));
        }
        if let Some(x) = c.write_errors_uncorrected {
            bits.push(format!("uncorrected write errors {x}"));
        }
        if let Some(x) = c.wear.filter(|x| *x > 0) {
            bits.push(format!("wear {x}%"));
        }
        if let Some(x) = c.power_on_hours {
            bits.push(format!("{x} h powered on"));
        }
        if !bits.is_empty() {
            counters = format!(" Windows also reports: {}.", bits.join(", "));
        }
    }
    let health = s(&f.health_status);
    let what = format!("{} - {} ({})", s(&f.friendly_name), health, s(&f.operational_status));

    let ev = f.disk_events.as_ref();
    let bad_blocks = ev.map_or(0, |e| e.bad_block);
    let paging = ev.map_or(0, |e| e.paging);
    let resets = ev.map_or(0, |e| e.reset);
    let when = match ev.and_then(|e| e.first.map(|first| (first, e.last))) {
        Some((first, last)) => format!(" between {first} and {}", last.map(|l| l.to_string()).unwrap_or_default()),
        None => String::new(),
    };
    let sm = f.smart.as_ref().filter(|m| eq_ci(s(&m.source), "ata-smart"));
    let over = |x: Option<i64>| x.filter(|v| *v > 0);
    let (mut media, mut link) = (Vec::new(), Vec::new());
    if let Some(m) = sm {
        if let Some(x) = over(m.uncorrectable) {
            media.push(format!("{x} uncorrectable read errors reported by the drive (SMART 187)"));
        }
        if let Some(x) = over(m.pending) {
            media.push(format!("{x} sectors pending reallocation (SMART 197)"));
        }
        if let Some(x) = over(m.reallocated) {
            media.push(format!("{x} sectors already reallocated (SMART 5)"));
        }
        if let Some(x) = over(m.crc) {
            link.push(format!("{x} interface CRC errors (SMART 199) - that one points at the cable or connector, not the flash"));
        }
    }
    let smart_line = if media.is_empty() && link.is_empty() {
        String::new()
    } else {
        let all: Vec<String> = media.iter().chain(&link).cloned().collect();
        format!(" The drive itself reports: {}.", all.join("; "))
    };
    let hard_media = sm.is_some_and(|m| over(m.uncorrectable).is_some() || over(m.pending).is_some());

    if !eq_ci(health, "Unhealthy") && (bad_blocks > 0 || hard_media) {
        let why = if bad_blocks > 0 { format!("Windows logged {bad_blocks} bad-block errors on this drive in the last 30 days{when}") } else { "the drive reports sectors it cannot read".to_string() };
        scan.add("Storage", T, Status::Fail, format!("{what} - {why}"))
            .note(format!("The drive reports HealthStatus {health}, but {why} - sectors the drive could not read or write, which is what a failing drive looks like before its own health flag trips.{smart_line}{counters} Converting on it risks losing files during the copy, and the new system would live on it."))
            .remedy("Do not convert on this drive. Copy your files off it now, while it still reads, then replace the drive and run this scanner again.");
        return;
    }
    if eq_ci(health, "Healthy") && (paging > 0 || resets > 0 || !media.is_empty() || !link.is_empty()) {
        let mut bits = Vec::new();
        if paging > 0 {
            bits.push(format!("{paging} paging errors"));
        }
        if resets > 0 {
            bits.push(format!("{resets} device resets"));
        }
        let ev_line = if bits.is_empty() { String::new() } else { format!(" Windows logged {} on this drive in the last 30 days{when}.", bits.join(" and ")) };
        let remedy = if !link.is_empty() && media.is_empty() {
            "Interface CRC errors usually mean a loose or dirty connector or a bad cable: reseat the drive, then run this scanner again. Copy your important files somewhere else first regardless."
        } else {
            "Copy your important files somewhere else FIRST. Then consider replacing the drive before converting."
        };
        scan.add("Storage", T, Status::Warn, format!("{what} - errors logged"))
            .note(format!("Windows' health flag for the drive is fine, but there are signs of trouble.{ev_line}{smart_line}{counters} The converter will not shrink this drive or run a disk check on it, so keeping Windows as a fallback is not offered."))
            .remedy(remedy);
        return;
    }
    if eq_ci(health, "Healthy") {
        scan.add("Storage", T, Status::Ok, what)
            .note(format!("Windows' own health check reports nothing wrong with the drive that holds Windows, and the last 30 days of its error log hold no bad-block, paging or reset events for it.{counters}"));
    } else if eq_ci(health, "Warning") {
        scan.add("Storage", T, Status::Warn, what)
            .note(format!("Windows reports a warning for the drive that holds Windows - it is showing early signs of trouble.{counters} The converter will not shrink this drive or run a disk check on it, so keeping Windows as a fallback is not offered; converting is still possible with your files on the USB stick (the clean-slate path)."))
            .remedy("Copy your important files somewhere else FIRST - a drive showing warnings can fail without further notice. Then consider replacing the drive before converting: a new drive makes the conversion simpler and the result more reliable.");
    } else if eq_ci(health, "Unhealthy") {
        scan.add("Storage", T, Status::Fail, what)
            .note(format!("Windows reports the drive that holds Windows as unhealthy - it is failing.{counters} Converting on a failing drive risks losing your files during the copy, and the new system would live on it."))
            .remedy("Do not convert on this drive. Copy your files off it now, replace the drive, then run this scanner again.");
    } else {
        scan.add("Storage", T, Status::Unknown, what)
            .note(format!("Windows reported a health status this scanner does not recognise ('{health}'). The converter reads this again and refuses the disk-check step unless it reads Healthy."));
    }
}

/// Test-UpgVolumeHealth. A flagged volume is not data loss and not a
/// refusal: it is something the keep-Windows path must clear first.
pub fn volume_health(scan: &mut Scan, is_admin: bool, health: Option<&VolumeHealth>) {
    const T: &str = "Volume health";
    if !is_admin {
        scan.add("Storage", T, Status::Info, "requires Administrator to check")
            .note("Whether Windows has flagged C: for a disk check needs Administrator rights to read. A flagged volume cannot be shrunk, so this matters for keeping Windows as a fallback.");
        return;
    }
    let no_data = VolumeHealth { dirty: Some("unknown".into()), error: Some("no data".into()), ..Default::default() };
    let h = health.unwrap_or(&no_data);
    let scan_text = s(&h.scan);
    let dirty = eq_ci(s(&h.dirty), "dirty");

    // anchored on purpose: 'NoErrorsFound' contains 'ErrorsFound'
    let scan_found_errors = truthy(&h.scan) && matches("^(ErrorsFound|ErrorsNotFixed)$", scan_text.trim());
    let repair_needed = matches("repair", s(&h.volume_status));
    let ntfs_full = ntfs98_fresh(Some(h));
    let lg = h.logged.as_ref();
    let log_found = lg.is_some_and(|l| eq_ci(&l.verdict, "found-problems"));
    if dirty || scan_found_errors || repair_needed || ntfs_full || log_found {
        let mut facts = Vec::new();
        if dirty {
            facts.push("C: is flagged for a disk check (dirty)".to_string());
        }
        if repair_needed {
            facts.push(format!("Windows reports the volume as '{}'", s(&h.volume_status)));
        }
        if ntfs_full {
            facts.push(format!("NTFS logged on {} that C: needs to be taken offline for a full chkdsk", h.ntfs_full_chkdsk.map(|t| t.to_string()).unwrap_or_default()));
        }
        if let Some(l) = lg.filter(|_| log_found) {
            let records = if l.records > 0 { format!(" ({} corruption records)", l.records) } else { String::new() };
            let queued = if l.queued > 0 { format!(", {} item(s) queued for offline repair", l.queued) } else { String::new() };
            facts.push(format!("Windows' last check log found problems{records}{queued}"));
        } else if scan_found_errors {
            facts.push(format!("online scan reported: {scan_text}"));
        }
        let detail = facts[0].clone();
        let real = repair_needed || ntfs_full || log_found || scan_found_errors;
        let cmdlet_line = if h.scan_ran && truthy(&h.scan) {
            let contradicts = if log_found && matches("^NoErrorsFound$", scan_text) { " - which its own log contradicts; the log decides" } else { "" };
            format!(" The Repair-Volume cmdlet answered '{scan_text}'{contradicts}.")
        } else {
            String::new()
        };
        let meaning = if real {
            " This is real filesystem damage that Windows has queued for an offline repair, not just a stale flag: the full chkdsk (/f) is the only rung that clears it, and files whose sectors cannot be read come out of that repair truncated or missing."
        } else {
            " The flag is usually left behind by an unclean shutdown or a crash; it is not by itself a sign that anything is lost."
        };
        let remedy = if real {
            "Copy your important files somewhere else FIRST. Then let Windows repair the volume: open an Administrator prompt, run \"chkdsk C: /f\", answer Y, restart. The converter runs that same full check itself, with its own restart, before it measures anything - it cannot skip it."
        } else {
            "Windows fixes this itself: open an Administrator prompt, run \"chkdsk C: /f\", answer Y so it runs at the next restart, then restart. The converter will do this step for you before it measures anything - it cannot skip it, because Windows will not shrink a flagged volume."
        };
        scan.add("Storage", T, Status::Warn, detail)
            .note(format!("Windows has marked this volume as needing a check and will refuse to measure or shrink it until that check has run - this is exactly why 'Room to keep Windows' could not be measured, if it could not. Facts: {}.{cmdlet_line}{meaning}", facts.join("; ")))
            .remedy(remedy);
        return;
    }
    if eq_ci(s(&h.dirty), "clean") {
        let mut extra = if h.scan_ran && truthy(&h.scan) { format!("; online scan: {scan_text}") } else { String::new() };
        if let (Some(asked), Some(done)) = (h.ntfs_full_chkdsk, h.last_check) {
            extra.push_str(&format!("; NTFS asked for a full check on {asked} and a boot-time check completed on {done}"));
        }
        scan.add("Storage", T, Status::Ok, format!("no disk check pending{extra}"));
        return;
    }
    let why = if truthy(&h.error) { format!(": {}", s(&h.error)) } else { String::new() };
    scan.add("Storage", T, Status::Unknown, "could not read the volume flag")
        .note(format!("fsutil did not answer in a form this scanner understands{why}."));
}

/// `hiberboot_enabled`: 1 on, 0 off, nothing = the key could not be read
/// (and then the check says nothing).
pub fn fast_startup(scan: &mut Scan, hiberboot_enabled: Option<i64>) {
    match hiberboot_enabled {
        Some(1) => {
            scan.add("Storage", "Fast Startup", Status::Warn, "enabled")
                .note("Windows Fast Startup does not fully shut down - it hibernates. That leaves the Windows partition in a state Linux will not write to, and makes resizing it unsafe. Shutting down does not clear it; only a restart or disabling the feature does.")
                .remedy("Control Panel > Power Options > Choose what the power buttons do > Change settings that are currently unavailable > untick \"Turn on fast startup\". Then shut down normally.");
        }
        Some(0) => {
            scan.add("Storage", "Fast Startup", Status::Ok, "disabled");
        }
        _ => {}
    }
}

pub fn bitlocker(scan: &mut Scan, is_admin: bool, state: Option<&BitLockerState>) {
    const T: &str = "BitLocker";
    if !is_admin {
        scan.add("Storage", T, Status::Unknown, "requires Administrator to check")
            .note("Could not determine whether this disk is encrypted. This matters more than any other unknown here: if BitLocker is on and you resize or reinstall without the recovery key, the data is gone permanently and no recovery is possible.")
            .remedy("Re-run this scanner as Administrator, or check manually: Settings > Privacy & security > Device encryption.");
        return;
    }
    match state.filter(|st| st.succeeded) {
        Some(st) if !st.encrypted_mounts.is_empty() => {
            scan.add("Storage", T, Status::Warn, format!("enabled on {}", st.encrypted_mounts.join(", ")))
                .note("This disk is encrypted. Any partition change without the recovery key destroys the data irrecoverably.")
                .remedy("Save your recovery key before doing anything: run \"manage-bde -protectors -get C:\" as Administrator, or retrieve it from account.microsoft.com/devicerecoverykey. Save it somewhere that is not this computer. Then either suspend or fully decrypt BitLocker before touching partitions.");
        }
        Some(_) => {
            scan.add("Storage", T, Status::Ok, "not enabled");
        }
        None => {
            scan.add("Storage", T, Status::Unknown, "query failed")
                .note("Could not read encryption status. Treat the disk as possibly encrypted until you have confirmed otherwise.")
                .remedy("Check Settings > Privacy & security > Device encryption before proceeding.");
        }
    }
}

/// Test-UpgEsp. R21's decision (2026-08-30): keeping Windows needs 32 MiB
/// free on the boot partition, and the firmware's Windows entry must point
/// at that same partition. Failing either steers to clean slate.
pub fn esp(scan: &mut Scan, is_admin: bool, facts: Option<&EspFacts>) {
    const T: &str = "Boot partition (ESP)";
    if !is_admin {
        scan.add("Storage", T, Status::Info, "requires Administrator to check")
            .note("Whether the small boot partition has room to add Linux alongside Windows needs Administrator rights to measure. The clean-slate path (files on the USB stick) works regardless.")
            .remedy("Re-run this scanner as Administrator to get this number.");
        return;
    }
    let Some(f) = facts.filter(|f| f.succeeded) else {
        scan.add("Storage", T, Status::Unknown, "could not read the EFI system partition")
            .note("The boot partition could not be mounted or measured, so there is no basis to promise the keep-Windows path. The converter re-checks before doing anything.");
        return;
    };
    let free_bytes = f.free_bytes.unwrap_or(0.0);
    let free = num(round1(free_bytes / MB));
    if f.has_windows_boot_files != Some(true) || f.bootmgr_points_at_esp == Some(false) {
        scan.add("Storage", T, Status::Warn, "Windows does not boot from the expected partition")
            .note("The partition this machine actually boots Windows from is not the one its boot files were found on (or those files are missing). Installing Linux alongside would touch a partition the firmware does not boot from, so the keep-Windows fallback cannot be promised here. The clean-slate path still converts this machine.");
        return;
    }
    if f.bootmgr_points_at_esp.is_none() {
        scan.add("Storage", T, Status::Unknown, format!("{free} MiB free, but the Windows boot entry could not be resolved"))
            .note("Could not confirm that the firmware boots Windows from this partition. Treat the keep-Windows path as unconfirmed; the converter re-checks before doing anything.");
        return;
    }
    if free_bytes < 32.0 * MB {
        scan.add("Storage", T, Status::Warn, format!("only {free} MiB free on the boot partition"))
            .note("Adding Linux alongside Windows puts its boot files on this partition (about 7 MB measured, and future kernel updates need headroom). Below 32 MiB free the keep-Windows path is not offered; the clean-slate path still works.")
            .remedy("Nothing to do by hand - do not resize or delete files on the boot partition yourself. The converter will steer this machine to the clean-slate path.");
        return;
    }
    scan.add("Storage", T, Status::Ok, format!("{free} MiB free; Windows boots from it"))
        .note("The boot partition has room to add Linux's boot files while keeping Windows bootable alongside.");
}
