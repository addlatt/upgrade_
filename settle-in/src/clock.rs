//! The clock (decided 2026-09-26; RISKS R28 for the portability).
//!
//! Windows keeps the hardware clock in local time; Linux reads it as UTC.
//! Found on the Aspire's run 9: the installer's clock was 4 hours behind.
//! On first startup, before the network, settle-in works out from evidence
//! whether the hardware clock still holds local time, and if it does, turns
//! it into UTC once, for good. Anything it cannot be sure of, it leaves
//! alone and says why: a wrong "correction" would move a right clock by
//! hours, and once online the time service corrects a wrong clock anyway.
//!
//! The evidence:
//!   - harvest.clock (Windows): the zone, whether the hardware clock held
//!     local time, and the offset Windows was using (H). That offset is the
//!     one baked into the hardware clock; nothing adjusts it after Windows.
//!   - cutover.clock (the installer, at the end of the install): the
//!     installer's clock, the hardware clock, and whether a time service
//!     had synchronized. A synchronized installer may have rewritten the
//!     hardware clock as UTC already.

use crate::civil::{iso_utc, Civil};
use crate::hw::Machine;
use crate::zone::Zone;
use serde_json::{json, Value};

/// Readings within this many seconds count as the same. Zone offsets differ
/// by at least 15 minutes, so 5 minutes cannot confuse two of them.
pub const TOLERANCE: i64 = 300;

#[derive(Debug, PartialEq)]
pub enum Decision {
    /// Nothing to correct; the reason in plain words.
    NotNeeded(String),
    /// The hardware clock holds local time at offset `baked` (seconds,
    /// local minus UTC). `installer_error` is how far the installer's clock
    /// was from UTC (to correct the install records in the report).
    Fix { baked: i64, installer_error: i64 },
    /// Cannot be sure; leave the clock alone.
    Refuse(String),
}

pub struct Evidence {
    pub rtc_is_local: bool,
    pub dst_auto_adjust: bool,
    pub harvest_offset: i64,
    pub observed_utc: i64,
    pub installer_utc: i64,
    pub rtc_at_install: Option<i64>,
    pub ntp_synchronized: Option<bool>,
}

pub fn evidence(job: &Value, outcome: &Value) -> Result<Evidence, String> {
    let h = job.pointer("/harvest/clock").ok_or("the job has no clock facts (job writer before 0.15.0)")?;
    let c = outcome.pointer("/cutover/clock").ok_or("the install record has no clock readings (outcome.sh before 0.4.0)")?;
    let b = |v: &Value, k: &str| v.get(k).and_then(Value::as_bool).ok_or(format!("clock fact '{}' is missing", k));
    let i = |v: &Value, k: &str| v.get(k).and_then(Value::as_i64).ok_or(format!("clock fact '{}' is missing", k));
    let observed = h.get("observed_utc").and_then(Value::as_str).and_then(crate::civil::parse_iso_utc).ok_or("clock fact 'observed_utc' is missing")?;
    Ok(Evidence {
        rtc_is_local: b(h, "rtc_is_local")?,
        dst_auto_adjust: b(h, "dst_auto_adjust")?,
        harvest_offset: i(h, "utc_offset_minutes")? * 60,
        observed_utc: observed,
        installer_utc: i(c, "installer_utc_epoch")?,
        rtc_at_install: c.get("rtc_epoch").and_then(Value::as_i64),
        ntp_synchronized: c.get("ntp_synchronized").and_then(Value::as_bool),
    })
}

fn hm(s: i64) -> String {
    let a = s.abs();
    format!("{}{} h {:02} min", if s < 0 { "-" } else { "" }, a / 3600, a % 3600 / 60)
}

/// Pure: what the evidence says about the hardware clock at the end of the install.
pub fn decide(e: &Evidence, zone: Option<&Zone>) -> Decision {
    if !e.rtc_is_local {
        return Decision::NotNeeded("Windows already kept the hardware clock in UTC".into());
    }
    let h = e.harvest_offset;
    if e.dst_auto_adjust {
        match zone.map(|z| z.offset_at(e.observed_utc)) {
            Some(Ok(o)) if o as i64 == h => {}
            Some(Ok(o)) => {
                return Decision::Refuse(format!(
                    "Windows' clock was {} from UTC, but this system's rules for the zone say {} at that moment; the zone may be the wrong one",
                    hm(h),
                    hm(o as i64)
                ))
            }
            Some(Err(x)) => return Decision::Refuse(format!("the zone's rules could not be read ({})", x)),
            None => return Decision::Refuse("this system has no rules for the zone Windows used".into()),
        }
    }
    if h.abs() <= TOLERANCE {
        return Decision::NotNeeded("Windows' local time was UTC, so the hardware clock already holds UTC".into());
    }
    let rtc = match e.rtc_at_install {
        Some(r) => r,
        None => return Decision::Refuse("the installer could not read the hardware clock, so what it holds is not known".into()),
    };
    let d = rtc - e.installer_utc;
    let (baked, installer_error) = if (d - h).abs() <= TOLERANCE {
        // the installer's clock was right (a time service or the hypervisor
        // set it) and the hardware clock was still local time
        (h, 0)
    } else if d.abs() <= TOLERANCE {
        match e.ntp_synchronized {
            Some(true) => {
                return Decision::NotNeeded(
                    "the installer's time service had already set the hardware clock to UTC".into(),
                )
            }
            // the installer took its clock from the hardware clock: both held local time
            Some(false) => (h, h),
            None => {
                return Decision::Refuse(
                    "the hardware clock and the installer's clock agreed, but whether a time service had set them is not known, so the hardware clock may hold either local time or UTC".into(),
                )
            }
        }
    } else {
        return Decision::Refuse(format!(
            "at the end of the install the hardware clock and the installer's clock were {} apart, which matches neither local time ({}) nor UTC",
            hm(d),
            hm(h)
        ));
    };
    // a change of daylight saving between Windows' last run and the install
    // would leave it unclear which offset Windows last wrote
    if e.dst_auto_adjust
        && let Some(z) = zone
        && z.offset_at(e.installer_utc - installer_error).ok() != Some(h as i32)
    {
        return Decision::Refuse(
            "daylight saving changed between Windows' last run and the install, so which offset the hardware clock holds is not certain".into(),
        );
    }
    Decision::Fix { baked, installer_error }
}

/// /etc/adjtime says whether the hardware clock is UTC or LOCAL (line 3;
/// util-linux and systemd both read it). Returns the new text if it said LOCAL.
pub fn adjtime_to_utc(text: &str) -> Option<String> {
    let mut lines: Vec<&str> = text.lines().collect();
    if lines.len() >= 3 && lines[2].trim() == "LOCAL" {
        lines[2] = "UTC";
        return Some(lines.join("\n") + "\n");
    }
    None
}

/// The whole step. Returns the report block. `created_utc` is when Windows
/// wrote the job, with Windows' own (network-set) clock: a lower bound on
/// the true time now.
pub fn run(m: &mut dyn Machine, root: &str, e: &Evidence, zone: Option<&Zone>, created_utc: i64, recorded: &Value) -> Value {
    let decision = decide(e, zone);
    let mut r = json!({ "decision": format!("{:?}", decision) });
    let (baked, installer_error) = match decision {
        Decision::NotNeeded(why) => return json!({ "result": "not-needed", "why": why }),
        Decision::Refuse(why) => return json!({ "result": "left-alone", "why": why }),
        Decision::Fix { baked, installer_error } => (baked, installer_error),
    };
    if m.kernel_synchronized() {
        return json!({ "result": "left-alone",
            "why": "a time service already set the clock during this startup; the hardware clock may already hold UTC" });
    }
    let now_rtc = match m.rtc_read() {
        Ok(c) if c.valid() => c,
        Ok(c) => return json!({ "result": "left-alone", "why": format!("the hardware clock holds an impossible date ({})", c.iso()) }),
        Err(x) => return json!({ "result": "left-alone", "why": x }),
    };
    let utc = now_rtc.as_unix() - baked;
    if utc < created_utc - TOLERANCE {
        return json!({ "result": "left-alone",
            "why": format!("the corrected time {} would be before Windows wrote the job ({}); something else is wrong", iso_utc(utc), iso_utc(created_utc)) });
    }
    let before = m.system_now();
    if let Err(x) = m.set_system_clock(utc) {
        return json!({ "result": "failed", "why": x });
    }
    let rtc_written = m.rtc_write(&Civil::from_unix(utc));
    let adj = format!("{}/etc/adjtime", root.trim_end_matches('/'));
    let adjtime = match std::fs::read_to_string(&adj) {
        Ok(t) => match adjtime_to_utc(&t) {
            Some(n) => match std::fs::write(&adj, n) {
                Ok(()) => "changed LOCAL to UTC".to_string(),
                Err(x) => format!("still says LOCAL, could not write it ({})", x),
            },
            None => "already UTC".to_string(),
        },
        Err(_) => "absent (the system reads the hardware clock as UTC)".to_string(),
    };
    r = json!({
        "result": if rtc_written.is_ok() { "corrected" } else { "system-clock-only" },
        "hardware_clock_was_local": now_rtc.iso(),
        "offset_it_held": hm(baked),
        "system_clock_before_utc": iso_utc(before),
        "system_clock_after_utc": iso_utc(utc),
        "hardware_clock_now": match &rtc_written { Ok(()) => "UTC".to_string(), Err(x) => format!("still local time: {}", x) },
        "adjtime": adjtime,
        "install_records": install_records(recorded, installer_error),
        "decision": r["decision"],
    });
    r
}

/// The installer's records, as recorded and as corrected. The records
/// themselves are never rewritten (decided 2026-09-26).
pub fn install_records(outcome: &Value, installer_error: i64) -> Value {
    let mut v = json!({ "installer_clock_error": hm(installer_error), "installer_clock_error_seconds": installer_error });
    for (name, ptr) in [("commit_line_crossed_utc", "/commit_line/crossed_utc"), ("outcome_created_utc", "/created_utc")] {
        if let Some(t) = outcome.pointer(ptr).and_then(Value::as_str).and_then(crate::civil::parse_iso_utc) {
            v[name] = json!({ "as_recorded": iso_utc(t), "corrected": iso_utc(t - installer_error) });
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = -4 * 3600; // New York in September
    fn ev(rtc: Option<i64>, inst: i64, ntp: Option<bool>) -> Evidence {
        Evidence { rtc_is_local: true, dst_auto_adjust: true, harvest_offset: H, observed_utc: 1790510400, installer_utc: inst, rtc_at_install: rtc, ntp_synchronized: ntp }
    }
    fn ny() -> Zone {
        Zone::load("", "America/New_York").unwrap()
    }
    const TRUE_INSTALL: i64 = 1790512000; // 2026-09-27 ~12:26 UTC

    #[test]
    fn the_aspire_case_installer_copied_the_local_hardware_clock() {
        // no network in the installer: its clock came from the hardware clock (local), 4 h behind
        let local_as_utc = TRUE_INSTALL + H;
        assert_eq!(decide(&ev(Some(local_as_utc + 2), local_as_utc, Some(false)), Some(&ny())), Decision::Fix { baked: H, installer_error: H });
        // agreeing clocks with no word on the time service could be either: left alone
        assert!(matches!(decide(&ev(Some(local_as_utc), local_as_utc, None), Some(&ny())), Decision::Refuse(_)));
    }

    #[test]
    fn installer_clock_right_hardware_clock_local() {
        // a hypervisor or a time service set the installer's clock, the hardware clock still local
        assert_eq!(decide(&ev(Some(TRUE_INSTALL + H), TRUE_INSTALL, Some(true)), Some(&ny())), Decision::Fix { baked: H, installer_error: 0 });
        assert_eq!(decide(&ev(Some(TRUE_INSTALL + H), TRUE_INSTALL, Some(false)), Some(&ny())), Decision::Fix { baked: H, installer_error: 0 });
    }

    #[test]
    fn synchronized_installer_already_wrote_utc() {
        assert!(matches!(decide(&ev(Some(TRUE_INSTALL), TRUE_INSTALL, Some(true)), Some(&ny())), Decision::NotNeeded(_)));
    }

    #[test]
    fn readings_that_match_nothing_are_left_alone() {
        assert!(matches!(decide(&ev(Some(TRUE_INSTALL + 7200), TRUE_INSTALL, Some(true)), Some(&ny())), Decision::Refuse(_)));
        assert!(matches!(decide(&ev(None, TRUE_INSTALL, Some(true)), Some(&ny())), Decision::Refuse(_)));
    }

    #[test]
    fn windows_in_utc_or_offset_zero_needs_nothing() {
        let mut e = ev(Some(TRUE_INSTALL), TRUE_INSTALL, None);
        e.rtc_is_local = false;
        assert!(matches!(decide(&e, Some(&ny())), Decision::NotNeeded(_)));
        let mut e = ev(Some(TRUE_INSTALL), TRUE_INSTALL, None);
        e.harvest_offset = 0;
        e.dst_auto_adjust = false;
        assert!(matches!(decide(&e, None), Decision::NotNeeded(_)));
    }

    #[test]
    fn zone_disagreeing_with_windows_is_refused() {
        let mut e = ev(Some(TRUE_INSTALL + H), TRUE_INSTALL, Some(true));
        e.harvest_offset = -5 * 3600; // not what New York is in September
        assert!(matches!(decide(&e, Some(&ny())), Decision::Refuse(_)));
        assert!(matches!(decide(&ev(Some(TRUE_INSTALL + H), TRUE_INSTALL, Some(true)), None), Decision::Refuse(_)));
    }

    #[test]
    fn daylight_saving_off_in_windows_uses_its_offset_not_the_rules() {
        let mut e = ev(Some(TRUE_INSTALL - 5 * 3600), TRUE_INSTALL, Some(true));
        e.harvest_offset = -5 * 3600;
        e.dst_auto_adjust = false;
        assert_eq!(decide(&e, Some(&ny())), Decision::Fix { baked: -5 * 3600, installer_error: 0 });
    }

    #[test]
    fn a_dst_change_between_windows_and_the_install_is_refused() {
        // harvested in summer (-4 h), installed a day after the November switch
        let nov2 = Civil { year: 2026, month: 11, day: 2, hour: 12, minute: 0, second: 0 }.as_unix(); // New York is -5 h
        let mut e = ev(Some(nov2 + H), nov2, Some(true));
        e.observed_utc = Civil { year: 2026, month: 10, day: 31, hour: 20, minute: 0, second: 0 }.as_unix(); // still -4 h
        assert_eq!(ny().offset_at(e.observed_utc).unwrap() as i64, H);
        assert_eq!(ny().offset_at(nov2).unwrap(), -5 * 3600);
        assert!(matches!(decide(&e, Some(&ny())), Decision::Refuse(_)));
    }

    #[test]
    fn adjtime_local_becomes_utc() {
        assert_eq!(adjtime_to_utc("0.0 0 0.0\n0\nLOCAL\n").unwrap(), "0.0 0 0.0\n0\nUTC\n");
        assert_eq!(adjtime_to_utc("0.0 0 0.0\n0\nUTC\n"), None);
    }

    struct Fake {
        rtc: Civil,
        sys: i64,
        synced: bool,
        wrote_rtc: Option<Civil>,
    }
    impl Machine for Fake {
        fn rtc_read(&mut self) -> Result<Civil, String> { Ok(self.rtc) }
        fn rtc_write(&mut self, t: &Civil) -> Result<(), String> { self.wrote_rtc = Some(*t); self.rtc = *t; Ok(()) }
        fn system_now(&mut self) -> i64 { self.sys }
        fn set_system_clock(&mut self, u: i64) -> Result<(), String> { self.sys = u; Ok(()) }
        fn kernel_synchronized(&mut self) -> bool { self.synced }
    }

    #[test]
    fn run_corrects_both_clocks_and_reports_the_records() {
        // first startup the next day: the hardware clock still reads local time, 4 h behind
        let truth = TRUE_INSTALL + 86400;
        let mut m = Fake { rtc: Civil::from_unix(truth + H), sys: truth + H, synced: false, wrote_rtc: None };
        let local_as_utc = TRUE_INSTALL + H;
        let outcome = json!({ "created_utc": iso_utc(local_as_utc), "commit_line": { "crossed_utc": iso_utc(local_as_utc - 600) } });
        let r = run(&mut m, "/nonexistent", &ev(Some(local_as_utc), local_as_utc, Some(false)), Some(&ny()), 1790509000, &outcome);
        assert_eq!(r["result"], "corrected");
        assert_eq!(m.sys, truth);
        assert_eq!(m.wrote_rtc, Some(Civil::from_unix(truth)));
        assert_eq!(r["install_records"]["commit_line_crossed_utc"]["corrected"], iso_utc(TRUE_INSTALL - 600));
        assert_eq!(r["install_records"]["commit_line_crossed_utc"]["as_recorded"], iso_utc(local_as_utc - 600));
    }

    #[test]
    fn run_leaves_the_clock_when_a_time_service_already_ran() {
        let mut m = Fake { rtc: Civil::from_unix(TRUE_INSTALL), sys: TRUE_INSTALL, synced: true, wrote_rtc: None };
        let r = run(&mut m, "/nonexistent", &ev(Some(TRUE_INSTALL + H), TRUE_INSTALL, Some(false)), Some(&ny()), 1790509000, &json!({}));
        assert_eq!(r["result"], "left-alone");
        assert_eq!(m.wrote_rtc, None);
    }

    #[test]
    fn run_refuses_a_time_before_the_job() {
        let mut m = Fake { rtc: Civil::from_unix(1700000000), sys: 1700000000, synced: false, wrote_rtc: None };
        let r = run(&mut m, "/nonexistent", &ev(Some(TRUE_INSTALL + H), TRUE_INSTALL, Some(false)), Some(&ny()), 1790509000, &json!({}));
        assert_eq!(r["result"], "left-alone");
        assert_eq!(m.wrote_rtc, None);
    }
}
