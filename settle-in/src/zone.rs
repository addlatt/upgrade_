//! A time zone's own rules, read from the installed system's zoneinfo file
//! (the tzdata every distribution ships). Used as a cross-check on the
//! offset Windows reported, never as the correction itself: the hardware
//! clock holds the offset Windows last wrote into it, and nothing moves it
//! when daylight saving changes after Windows has gone.

use tz::TimeZone;

pub struct Zone {
    tz: TimeZone,
}

impl Zone {
    pub fn load(root: &str, iana: &str) -> Result<Zone, String> {
        let ok = !iana.is_empty()
            && !iana.starts_with('/')
            && iana.split('/').all(|p| !p.is_empty() && p != "." && p != "..")
            && iana.chars().all(|c| c.is_ascii_alphanumeric() || "/_-+".contains(c));
        if !ok {
            return Err(format!("'{}' is not a time zone name", iana));
        }
        let path = format!("{}/usr/share/zoneinfo/{}", root.trim_end_matches('/'), iana);
        let bytes = std::fs::read(&path).map_err(|e| format!("this system has no rules for {} ({}: {})", iana, path, e))?;
        let tz = TimeZone::from_tz_data(&bytes).map_err(|e| format!("{} is not a readable zone file ({})", path, e))?;
        Ok(Zone { tz })
    }

    /// Local time minus UTC, in seconds, at this instant.
    pub fn offset_at(&self, unix: i64) -> Result<i32, String> {
        self.tz.find_local_time_type(unix).map(|t| t.ut_offset()).map_err(|e| format!("{}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civil::Civil;

    #[test]
    fn summer_and_winter_offsets() {
        let z = Zone::load("", "America/New_York").expect("the build machine has tzdata");
        assert_eq!(z.offset_at(Civil { year: 2026, month: 9, day: 27, hour: 12, minute: 0, second: 0 }.as_unix()).unwrap(), -4 * 3600);
        assert_eq!(z.offset_at(Civil { year: 2026, month: 1, day: 15, hour: 12, minute: 0, second: 0 }.as_unix()).unwrap(), -5 * 3600);
    }

    #[test]
    fn refuses_a_path_for_a_name() {
        assert!(Zone::load("", "../../etc/passwd").is_err());
        assert!(Zone::load("", "/etc/passwd").is_err());
    }
}
