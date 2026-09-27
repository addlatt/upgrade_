//! The machine, behind one interface. Everything settle-in does to the
//! hardware clock and the system clock goes through `Machine`, so the
//! deciding code can be tested against a fake one (rule #5, logic level)
//! and the real one is a few kernel calls, the same on every distribution:
//! the RTC through /dev/rtc0 (no `hwclock`), the system clock through
//! clock_settime, and the kernel's own "synchronized" flag through adjtimex.

use crate::civil::Civil;
use std::fs::File;
use std::os::fd::AsRawFd;

pub trait Machine {
    /// The hardware clock's fields, exactly as it holds them (no zone applied).
    fn rtc_read(&mut self) -> Result<Civil, String>;
    /// Store these fields in the hardware clock.
    fn rtc_write(&mut self, t: &Civil) -> Result<(), String>;
    /// The system clock, seconds since 1970 UTC.
    fn system_now(&mut self) -> i64;
    fn set_system_clock(&mut self, unix: i64) -> Result<(), String>;
    /// True when the kernel says a time service has synchronized it. Then
    /// the kernel copies the system clock into the hardware clock every 11
    /// minutes, so the hardware clock may already hold UTC.
    fn kernel_synchronized(&mut self) -> bool;
}

// struct rtc_time from <linux/rtc.h>: nine ints
#[repr(C)]
#[derive(Default)]
struct RtcTime {
    tm_sec: i32,
    tm_min: i32,
    tm_hour: i32,
    tm_mday: i32,
    tm_mon: i32,  // 0-11
    tm_year: i32, // years since 1900
    tm_wday: i32,
    tm_yday: i32,
    tm_isdst: i32,
}
const RTC_RD_TIME: libc::c_ulong = 0x8024_7009; // _IOR('p', 0x09, struct rtc_time)
const RTC_SET_TIME: libc::c_ulong = 0x4024_700a; // _IOW('p', 0x0a, struct rtc_time)
const STA_UNSYNC: i32 = 0x0040;

pub struct Real {
    pub rtc_path: String,
}

impl Machine for Real {
    fn rtc_read(&mut self) -> Result<Civil, String> {
        let f = File::open(&self.rtc_path).map_err(|e| format!("{}: {}", self.rtc_path, e))?;
        let mut t = RtcTime::default();
        // SAFETY: RTC_RD_TIME fills a struct rtc_time, which RtcTime mirrors.
        let rc = unsafe { libc::ioctl(f.as_raw_fd(), RTC_RD_TIME as _, &mut t as *mut RtcTime) };
        if rc != 0 {
            return Err(format!("reading {} failed: {}", self.rtc_path, std::io::Error::last_os_error()));
        }
        Ok(Civil {
            year: t.tm_year as i64 + 1900,
            month: t.tm_mon as u32 + 1,
            day: t.tm_mday as u32,
            hour: t.tm_hour as u32,
            minute: t.tm_min as u32,
            second: t.tm_sec as u32,
        })
    }

    fn rtc_write(&mut self, c: &Civil) -> Result<(), String> {
        let f = File::options().write(true).open(&self.rtc_path).map_err(|e| format!("{}: {}", self.rtc_path, e))?;
        let t = RtcTime {
            tm_sec: c.second as i32,
            tm_min: c.minute as i32,
            tm_hour: c.hour as i32,
            tm_mday: c.day as i32,
            tm_mon: c.month as i32 - 1,
            tm_year: (c.year - 1900) as i32,
            ..Default::default()
        };
        // SAFETY: RTC_SET_TIME reads a struct rtc_time, which RtcTime mirrors.
        let rc = unsafe { libc::ioctl(f.as_raw_fd(), RTC_SET_TIME as _, &t as *const RtcTime) };
        if rc != 0 {
            return Err(format!("writing {} failed: {}", self.rtc_path, std::io::Error::last_os_error()));
        }
        Ok(())
    }

    fn system_now(&mut self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    fn set_system_clock(&mut self, unix: i64) -> Result<(), String> {
        let ts = libc::timespec { tv_sec: unix as _, tv_nsec: 0 };
        // SAFETY: a valid timespec, CLOCK_REALTIME.
        let rc = unsafe { libc::clock_settime(libc::CLOCK_REALTIME, &ts) };
        if rc != 0 {
            return Err(format!("setting the system clock failed: {}", std::io::Error::last_os_error()));
        }
        Ok(())
    }

    fn kernel_synchronized(&mut self) -> bool {
        // SAFETY: modes = 0 only reads the kernel's time state.
        let mut tx: libc::timex = unsafe { std::mem::zeroed() };
        let state = unsafe { libc::adjtimex(&mut tx) };
        state >= 0 && state != libc::TIME_ERROR && (tx.status & STA_UNSYNC) == 0
    }
}
