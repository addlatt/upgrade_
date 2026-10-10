//! One whole scan: every check, in the scanner's order, then the verdict.
//! This is the PowerShell scanner's main section with the reads taken out.

use crate::check::Scan;
use crate::data::tables;
use crate::facts::Machine;
use crate::ps::{s, truthy, Version};
use crate::verdict::{recommendation, required_kernel, verdict, Recommendation, Verdict};
use crate::{hardware, parse, sbat, software, storage, system};

pub struct Outcome {
    pub scan: Scan,
    pub verdict: Verdict,
    pub required_kernel: Option<Version>,
    pub recommendation: Recommendation,
}

pub fn scan(m: &Machine) -> Outcome {
    let mut sc = Scan::new();
    let admin = m.is_admin;
    system::architecture(&mut sc, &m.sys);
    system::memory(&mut sc, &m.sys);
    system::firmware(&mut sc, &m.sys, m.secure_boot);
    sbat::judge_sbat(&mut sc, m.secure_boot, &m.sbat.levels, &m.sbat.files);
    let parsed: Vec<sbat::SbatMap> = m.sbat.levels.iter().filter(|l| truthy(&l.text)).map(|l| sbat::parse(s(&l.text))).collect();
    let level = sbat::merge(&parsed);
    // the firmware's key list is only read when elevated
    let db = if admin { m.db_authorities.as_deref() } else { None };
    sbat::judge_releases(&mut sc, m.secure_boot, &level, db, &tables().releases);
    system::resume(&mut sc, m.resume.as_ref());
    storage::storage_mode(&mut sc, &m.pnp);
    storage::disk(&mut sc, &m.disk, admin, parse::repair_queued(m.volume_health.as_ref()));
    storage::physical_disk(&mut sc, m.physical_disk.as_ref());
    storage::volume_health(&mut sc, admin, m.volume_health.as_ref());
    storage::fast_startup(&mut sc, m.hiberboot);
    storage::bitlocker(&mut sc, admin, if admin { m.bit_locker.as_ref() } else { None });
    storage::esp(&mut sc, admin, if admin { m.esp.as_ref() } else { None });
    hardware::wifi(&mut sc, &m.pnp);
    hardware::gpu(&mut sc, &m.pnp);
    hardware::audio(&mut sc, &m.pnp);
    hardware::vendor(&mut sc, &m.sys);
    software::apps(&mut sc, &m.apps);
    system::current_os(&mut sc, &m.sys);

    let required_kernel = required_kernel(&sc);
    let verdict = verdict(&sc);
    let recommendation = recommendation(&sc, required_kernel);
    Outcome { scan: sc, verdict, required_kernel, recommendation }
}

/// The checks a hardware-only recording can replay: the seven that read
/// nothing but the device list (the same seven the self-test replays).
pub fn scan_hardware_only(m: &Machine) -> Outcome {
    let mut sc = Scan::new();
    system::architecture(&mut sc, &m.sys);
    system::memory(&mut sc, &m.sys);
    storage::storage_mode(&mut sc, &m.pnp);
    hardware::wifi(&mut sc, &m.pnp);
    hardware::gpu(&mut sc, &m.pnp);
    hardware::audio(&mut sc, &m.pnp);
    hardware::vendor(&mut sc, &m.sys);
    let required_kernel = required_kernel(&sc);
    let verdict = verdict(&sc);
    let recommendation = recommendation(&sc, required_kernel);
    Outcome { scan: sc, verdict, required_kernel, recommendation }
}
